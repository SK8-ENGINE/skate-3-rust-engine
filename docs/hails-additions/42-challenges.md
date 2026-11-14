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
  stored bounds on two axes (center matching), so it is kept and flagged `bounds_mismatch`. Which of the two retail
  tests (box, bounds or both) is open; the engine port decides from retail code.
- Kept DLC maps keep their challenge rows across a refresh without the DLC folder (`retained.py`).
- Result on the disc: 16 own the spots on 3 maps; `ots_ind_01` is left out (no StartLocation, a leftover beside
  `ots_indu_01..05`). Export time 0.9 s; two runs give identical output.

Challenge parameters (research b847): the local data row (class `Hash_46A56ADDF284C381`) inherits from the
framework vault `data/db/challenge_local_data_framework` (family row, e.g. own_the_spots `BF65AB4C87F0A118`, then
`316A03931530B5E6`, then `default`). Exported per challenge: `challenge_type` (the bank's eChallengeTypes, own the spot 9; picks the sign-up overlay state), `out_of_bounds_time` (Int32 seconds),
`owned_display_time` / `killed_display_time` (the banner times), `return_to_area_message` (a text id). All 16 own
the spots: 5, 3.1, 3.3, `ID_CHALLENGE_MODIFIEDCENTRALMESSAGE_OTS_LEFTAREA`.

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
- timers (`challenges::timers`) hold trunc(seconds * 60) ticks; each 60 Hz tick first expires a timer at zero or
  below, else takes one tick off, so an N-tick timer expires on tick N + 1 (82D57960, research b956 / b958);
- the target is the owned score until the personal best reaches it, then the killed it score, then the personal
  best; a win completes the challenge only when that tier moves up, otherwise the challenge loops back to the start;
- not enough points resets the objectives and freezes scoring 0.5 s (no fail); a bail pauses scoring and resets on
  wipeout over; leaving the boundary starts the out-of-bounds countdown (expiry abandons), coming back cancels it;
- the killed it / owned it banner timers read the two display times crossed, as retail does;
- `Record` keeps the best completion, which only moves up (retail 82C1D878).
Not ported yet: SimulPlay, online scores, the intro presentation, highlight groups. The two display times and the
out-of-bounds time are challenge parameters, not bank fields (see the export section; research b847).

## Change: own the spot in play (`crates/skate-game/src/challenges.rs`)

- Loads `private/challenges.json` once; on each map, a run started from the sign-up (below) or with
  `SKATE_CHALLENGE=<id>` (dev start) arms the challenge's three volumes, starts `ots::Ots` with the session's best score,
  and each frame feeds it the player's trigger enter / exit events, the banked sequences and 60 Hz timer ticks.
- Points (retail rule, research b841): retail posts `cMsgSequenceComplete` (SequenceScore, Wipedout,
  UsedSessionMarker) when a sequence ends. Our scoring runtime now queues every banked sequence
  (`scoring_runtime::BankedSequence`, at the bank point beside `SCORING_BANK`); a sequence counts only when scoring
  is on, it did not end in a bail, and the player entered the spot volume during it; the spot clears after every
  sequence (activation mode 5). The group total against the current target gives all objectives complete or not
  enough points.
- Commands: teleport to the start locator (`travel_ex`, on board), objectives reset / evaluation / requirement, the
  completion recorded per challenge for the session (the tier only moves up), the HUD (below). The HUD counts as
  loaded at once and the highlight reel completes at once.
- Leaving the discovery sphere (distance from the challenge locator > `discovery_radius`, checked every frame as
  retail 8255BEB8 does) sends exit discovery, which abandons the run.
- The out-of-bounds time and the banner times come from the export's challenge parameters (own the spot: 5 s,
  3.1 s owned it, 3.3 s killed it); an export from before them uses the framework `default` row's values, which
  are the same.

## Change: the sign-up (`challenges.rs` `sign_up`)

Retail (research b842 / b843, corrected by b848 / b849): the challenge system shows a challenge's sign-up inside the
`GlobalActivateRadius` sphere (10 m for own the spot) around its `Location` locator. The "signup" HUD element's mode
picks the action overlay (`hud2/actionoverlay2`) state (82665E88): mode 0 (challenge sign-up) with one challenge of
`eChallengeTypes` 9 (own the spot; the family row's `ChallengeType` is 9) gives the OTS state 5 (its `ots_intro`,
"Activate Spot"), any other challenge list the Challenge state 1 ("Sign up", the title, the `DPad_Up_hud` glyph);
mode 1 is not a challenge: it is the grabbed object's actions (HUD `UpdateObjectGrabbing`, 82773788; its press
826660F0 plays `sp_upright_object` and posts 0x905C8249 = `cMsgUprightDMO`), shown as state 4; mode 2 the teleporter
(state 2, then a confirm popup). The challenge element is the SignUpManager's (82D52758, HUD
`cChallengeSignUpManager-DPadUp`): its list is the challenges whose 10 m sphere holds the player (82D53B38); the
D-pad Up press (826660F0 mode 0 -> 82D52D68) signs up with no popup and no sound, and an own the spot then starts
(82D52B60). Research b850.
- Not ported: several challenges in range (retail lists up to 4 in a pick popup; ours takes the nearest, own the
  spot spheres do not overlap on the disc maps), the Disabled state (mode 3, a listener refusing), the 15-frame
  re-press delay, the sign-up lock message 0x7949DAC1 (name unresolved).
- Ours: with no run going and gameplay up, the nearest challenge whose sphere holds the skater's feet shows the
  prompt: `PopupMods.signup_state` (`sign_up_state`: 5 for own the spot, else 1) and its title (text id) for the
  stage, which sends the overlay `SetScreenState`; teleporters now send their state 2 the same way and keep the overlay when both apply. D-pad Up
  (not with the session marker modifier) writes `ChallengeSignUp { body, challenge }` (a serializable message; the
  authority starts the run), which starts it as `SKATE_CHALLENGE` does. Without the stage, a text line shows.
- Order: the sign-up runs after the teleporter prompt and before the menu stage (`MenuStageSet`), so the stage reads
  one frame's final prompt.
- `--verify` runs: `SKATE_VERIFY_CHALLENGE=<id>` places the skater at the challenge's locator and presses for them
  once the prompt has shown 30 frames.
- Several challenges in range (research b851, checked by main in 826660F0): retail lists every challenge whose sphere
  holds the player, create-a-spot (eChallengeTypes 35) last in query order and the others before them in reverse
  order (82D29C38). The overlay then shows the Challenge state 1 with `ID_SIGNUP_MULTIPLIESIGNUP_SIGNUPHUD`
  ("Multiple Challenges", 82665DA0), and D-pad Up opens a popup (82D0A8D0): that title, an empty body (`#`), up to 4
  entries labeled by each challenge's Title id (create-a-spot: `#CreateASpot Needs A Name!`), cancelable; a pick
  signs up for that challenge (82D52D68, no second confirm), B starts nothing; no sound on the press path.
- The prompt's show and the re-press delay (research b854, checked by main in Ghidra): the prompt component's show
  (82665CD0) plays the fe sound `object_r1_options` (hash 0xFD2DC6E9E25D123D) every time the prompt appears, any
  prompt kind (the component is shared, so the teleporter prompt too), and sets a 15-tick delay (+0x7C) during which a
  press is ignored; hide (82665D48) clears it. Ours: the menu stage plays the sound when a sign-up state appears
  (`SIGNUP_SHOW_SOUND`), and `REPRESS_DELAY_SECONDS` (15 at 30 a second) holds the challenge press. Not ported:
  retail's early press also blocks further presses until an input bit 0x10000 arrives (probably the release; not
  traced), the teleporter prompt's delay, and the Disabled state (mode 3, overlay state 6, `ID_MESSAGE_PLEASE_WAIT`,
  D-pad Up does nothing): the gate 82D53058 asks registered listeners (research b856, not yet checked by main):
  16 challenge-system managers, 10 of which always say yes (the teleport manager too); the vetoes are the
  presentation director (a challenge presentation playing or queued), the Hall of Meat manager (two skater flags),
  BEManager (a screen layer, a central HUD flag and two globals), the living world manager (an unidentified world
  system), a game mode sub-object and probably the progression manager (a queued progression event). None checks a
  running challenge, air / off board, online or the replay. A port: a veto list where systems (and mods, through
  `sdk.challenges`) register "not now" answers by stable id, plus the separate show / hide owner hook.
  Ours: `in_sign_up` returns that list (query order = the exported list order, since retail's sphere slot order is not
  traced), `pick_popup` builds the request as an engine popup (`engine:challenge.pick`), and the pick is resolved by
  the challenge id offered at that entry (retail reads its live list by index, which could change while the popup is
  open). Without the menu stage the first entry starts. None of the 16 exported own the spots overlap (nearest pair
  over 40 m apart), so this matters for other families and mod challenges.

## Change: the challenge HUD in play (`hud_challenge.rs`, `scoring_hud.rs`)

Retail (research b844 / b845): the HUD component movies only read. Their `HUDComponents.*` natives return fields of
named HUD elements (`timer`, `dpo`, `objective`) that the graph's HUD2 bindings write; each element sends its movie
SetState 1 when it shows and 2 when it hides (vtable +0x34 / +0x38), and its update state on refresh.
- `ChallengeHudState` (a serializable resource: shown components, timer text and color flag, two DPO slots,
  objective rows) is what the elements hold. challenges.rs writes it from the run's commands: `ShowHud`,
  `StartTimerHud` (shows the timer), `UpdateHud` (DPO 0 = score / target titled `ID_COMMON_TARGET`, DPO 1 = score /
  personal best titled `ID_DPO_PERSONALBEST`, objective row 0 = `ID_CHALLENGE_OBJECTIVE_BEAT_OWNSPOT` /
  `_BEAT_KILLSPOT` / `_BEAT_PERSONALSCORE`), and the new personal best central message
  (`ID_LEADERBOARD_NEW_PERSONAL_BEST`, green). The end of a run or a map change clears it.
- The timer element (82663940): the out-of-bounds countdown's remaining time rounded up, at least 0, `M:SS` from a
  minute up and bare seconds below; the color flag drops to 0 (red) under 10 s (0x821963E4); under 10 s each new
  second sends SetState 4 (flash) and plays the `timer_end` front-end sound (event 0xAABBE111A2855FF0 =
  hash64("timer_end"), research b848), rising back sends 5.
- `hud_challenge::Runtime` runs each movie on its own APT VM (like the central HUD) and answers its natives from the
  state: `Timer_GetData`, `DPO_GetParams` (5 items per DPO) / `DPO_IsUsingTwoDPOs`, the `Objective_*` set (own the
  spot has no activity line), `FELanguage`, `Mission.IsInSkateParkTutorial`, `Audio.PlaySound` (front-end sounds,
  e.g. the banners' `challenge_ots` / `killedit_intro`), `Render.SetEffect`, `Mission.SetKilledItShakeActivation`
  (recorded; the shake is not ported). `sync` turns a state change into the movie's SetState: show -> 1 then its
  refresh state (objective: 100 first), hide -> 2, a change of what it reads while shown -> its refresh state
  (timer / dpo / objective 3).
- The scoring HUD loads every installed one (optional, like the minimap), resets them per map, syncs and updates
  them each frame and draws a shown movie, and a hidden one for 2 s of outro, between the minimap and the central
  messages.
- The personal best message is green: `HUD2.eMessageColour` is White 0, Red 1, Green 2 (registered by 8284DC78,
  research b847), and the color goes through 82850618 as the style, so style 2 (the movie's celebratory style).
- Leaving the boundary shows the challenge's `CentralMessageHALIDReturnToChallengeArea` (own the spot:
  `ID_CHALLENGE_MODIFIEDCENTRALMESSAGE_OTS_LEFTAREA`, "Deactivating Spot in", with the 5 s timer beside it). Retail
  hashes the id at run time with the language label hash (x33 from 0xFFFFFFFF, 826640F0) and shows it in the
  modified message slot (`ShowModifiedCentralMessage` 82850770: a second slot beside the normal message, screen
  event 10, hidden after 6 s by event 11).
- The modified slot (research b852, `hud_central.rs`): `CentralMessage::modified` shows it with screen event 10, its
  own 6 s timer hides it with 11, and `Central_GetModifiedMessageData` answers text, style, 0, 0. The movie's
  `mcModifiedMessage` clip places it (lower and smaller than the normal one, no sound, gone at once on hide) and, like
  every style 0 / 1 message, plays the green banner's outro. Own the spot never clears it early: returning to the
  area leaves it up its 6 s. Not ported: ClearModifiedCentralMessage (event 88; nothing in own the spot calls it).
- NOT RETAIL YET: the refresh cadence (retail refreshes the timer every 5th element tick; ours refreshes
  on any change of what the movie reads, which shows the same text).

## Change: the records kept across sessions (`challenge_records.rs`)

Retail (research b846): the career save's `Observatory::ChallengeStatusData` (v3; writer 82C1CF98, reader 82C1D260)
keeps per challenge the Owned and Killed key lists (exclusive, upgrade only, 82C1D878; Abandoned never stored) and a
key -> f32 high score map (`GetHighScore` 827D0F98 gives 0 with no entry; `SetHighScore` 827D1108 writes as told,
the graph compares first); `cChallengeDiscoveryManager` (v1) keeps key -> status (NotDiscovered 0, Discovered 1, set
by the engine when the player comes near, 8255BD38; SeenIntro 2, set by the graph). Keys are a 64-bit hash of the
challenge name.
- Ours: `settings/challenges.json` beside the install (`skate3rust.challenge-records` v1), keyed by our challenge
  id string: `completion` ("owned" / "killed"), `high_score`, `discovery`. Loaded with the challenges; the run starts
  with the saved high score and skips the intro when SeenIntro; `SetHighScore`, `MarkIntroSeen`, the completion and a
  first discovery (entering the challenge's boundary volume, as retail's 8255BD38 does on cMsgTriggerEnterCollision;
  research b850: every challenge boundary on the map is armed for it)
  change it, and each change saves at once on a short-lived thread (atomic `.part` then rename, like
  `career-stats.json`). A missing or bad file loads as no records.
- Mods: string ids, so a mod's challenges keep their own records; records of challenges not loaded (a disabled mod)
  stay in the file. Multiplayer: records are per player; the authority applies completions, each client saves its
  own player's.
- Not ported: the active / ever activated / flagged lists (+0x04 / +0x60 / +0x34; their readers are not traced), the
  progression unlock counts (82C1C698 / 82C1CA48).

## Change: the Challenges opt-in (`living_world/opt_ins.rs`)

The challenges are off by default on hails-additions until they are finished: a **Challenges** row in the Graphics
overlay's Opt-Ins (saved with the other opt-ins), or `SKATE_CHALLENGES=1` for a run (`SKATE_CHALLENGE=<id>` and
`SKATE_VERIFY_CHALLENGE=<id>` turn it on too). While it is off nothing arms, no prompt shows and nothing is
discovered or saved; switching it off mid-run drops the run, its HUD and the prompt, and switching it on arms the map
again. The session log shows `OPT_IN_CHANGED challenges ...` like the other opt-ins. Not retail: an opt-in of ours.

## Change: the mod entry (`modding/challenges.rs`, `sdk.challenges`)

Lua SDK API 2, capability `challenges` 1 (reference: `sdk/ENGINE_API.md` "Challenges"): the map's challenges with
their saved records, the run going, `start` (the same sign-up message the D-pad press sends), `abort` (retail's
Abandon event), `set_enabled` (hides a challenge's sign-up and discovery) and `configure` (per-mod overrides of the
setup values: owned / killed points, out of bounds time, the owned / killed banner times; applied at the next start in
mod id order). Events: discovered, started, personal best, ended. Every hide and override belongs to one mod and goes
when it stops. Multiplayer: the commands act on the local player's challenge; records stay per player.

## Decision: objectives as data, not Lua

Retail keeps each objective's completion condition as a small Lua predicate (`ObjectiveDefinition`, 1,606 rows, 681
distinct scripts; research b860) calling a native objective API (research b864, b863, b865). The port does not run
retail Lua: setup translates each predicate into a data expression the Rust objective runtime evaluates against the
same API, as the challenge state graphs are rewritten in Rust (decided 2026-11-06). Mods write objectives in the same
data form.

The group runtime (`crates/skate-core/src/challenges/objectives.rs`): objective states (inactive 1, armed 2, active 4,
complete 8, failed 0x10), activation modes 0 / volume (2, 3) / wipeout (4) / sequence (5), the records conditions
read (entered volumes since activation, refilled with the volumes the player is in; entered this sequence; tricks done
while active; session marker used / set, cleared by the next group tick after its evaluation pass, research
b894; 32 entries each, oldest dropped), the
`ObjectiveTriggers` mask byte per message (b864) with the second pass (byte 4) after a completion, the per-sequence
and per-wipeout resets, Flow (activate / deactivate / reset / status) and Reset (reset all / fail all). The condition
is a callback until the translated predicate form lands (research b867). Not wired into play yet. The translated form
(`crates/skate-core/src/challenges/predicate.rs`, research b867): values (nil, boolean, number, string, list),
expressions and statements for the subset the 3,795 parseable retail scripts use (if chains, locals, `vars`, globals,
and / or returning operands with short-circuit, comparisons and + / -, numeric for, API calls by resolved index),
evaluated with Lua 5.1 semantics on 32-bit float numbers (retail's Lua build, research b869); an error, a non-boolean
or no result gives the condition's default (completion conditions true, PointsValid / failure / hint false, set by
kind in 82C726E8), and writes made before an error stay. So a completion script with a retail syntax error or the
`DidSpecificTrickgroup` typo completes on its first evaluation, as in retail. `InitVar` is retail's `InitVars` prelude (one line of Lua in the XEX: set the field only when it is nil;
research b871). An objective's `vars` survive Flow.ResetObjective and are cleared only on the group's restart paths
(82C6F430 / 82C6D460) and teardown.

The translator (`tools/asset_pipeline/objective_lua.py`, tests `test_objective_lua.py` on synthetic scripts only):
a tokenizer and recursive-descent parser for the subset, emitting a versioned JSON tree (`skate3rust.objective-program`
v1) with names kept as strings for the game to resolve; `InitVars(vars, "name", value)` becomes `init_var`; known enum
paths become numbers (b869 values), others stay named; a syntax error becomes a program that errors when run (retail
never compiles it and returns the default). Checked locally against the user's bank (counts only, nothing kept): 1,712
of the 1,714 completion texts translate, 1 is empty, and the one failure is `hom_indu_05_0`, a real retail syntax error.
The export writes each challenge's `objectives` ({owned, killed}: per objective row its key and translated
`condition`; `objective_programs`, test in `test_challenges.py`), plus each objective's `points_valid` program (field
hash64("PointsValid"), inherited from the family rows and read from the bank the defining row came from; absent =
retail's default false). The loader `crates/skate-data/src/objective_program.rs` turns a program into the evaluator's
form, interning API, `vars` and global names (tests). The export also writes each row's `triggers` (tObjectiveTriggers
bytes 0..13) and `activation` (eTriggerActivationMode), inherited.

In play (`crates/skate-game/src/challenge_objectives.rs`): a running own the spot builds its objective group from the
owned rows (activation, trigger bytes, completion and PointsValid programs) and evaluates them through the objective
API (`crates/skate-core/src/challenges/api.rs`: EnteredVolume, IsSequenceNewlyCompletePoints,
GetGroupAccumulatedPoints, GetGroupPointsRequirement, GetAccumulatedPoints; matched by the last name segment; any
other call errors, so its condition gives the default). The group follows b841's order on each banked line: the
first pass, PointsValid adding the line's score to an objective that just completed, the second pass when something
completed, then the per-sequence reset; a bail or a session-marker line is not evaluated. Its result becomes the state
machine's AllObjectivesComplete / NotEnoughPoints. An export without objectives, or a row whose program does not load,
keeps the native check. Which group retail swaps in for the killed it target is not read: the owned rows run for
both [inferred; the two sets have the same shape]. Failure and hint rows: see the tutorials section; local-table fields (the shared
world-script variables of three Hall of Meat rows and one park tutorial) translate as indexes the engine cannot answer
yet, so those conditions give their default. Side effects a condition
has on its group (Flow, Reset, fraction counters) still need a command path from the API into `objectives.rs`.

## Change: empty arrays at the end of a vault (`tools/asset_pipeline/vlt.py`)

Seven retail challenge local vaults (hom_dwtn_05_sp, hom_indu_03 / 04 / 05, street_indu_02, vert_indu_02 / 03) failed
to convert with "Truncated AttribSys record": each ends with an empty array field (an 8-byte header, capacity 0) as the
last value in the .bin, and the converter's legacy `data` read took the element size after it. `field_bytes` reads
only the bytes that exist for an array field; every vault that converted before gives the same output (only reads
that raised change), and the seven now convert (1 to 13 rows each). Test: `test_vlt.py`. Setup-bench proof of
identical output for the full setup is still to run (not while the user plays).

## Change: background test windows (`startup_loading.rs`, `app.rs`, `splash.rs`)

`SKATE_BACKGROUND_WINDOW=1` opens the game window without taking focus: created hidden-to-inactive at the bottom of the
window order (`WindowLevel::AlwaysOnBottom`), never focused, no splash. Automated `--verify` runs use it (with `--mute`
and `SKATE_REPORT_CHILD=1`) so a test run never covers what the user has open. Not a player setting.

## Change: tutorials (research b858; in progress)

- State machine `crates/skate-core/src/challenges/tutorial.rs` from the retail `tutorials` graph (b858 §1): make the
  HUD, objectives, intro presentation, setup, asset load, running (reposition beyond 10 m unless the row says
  `IgnoreInitialTutorialReposition` false, which always repositions; no row sets it), results (3.5 s), outro, end
  options (continue / retry), restart, cleanup. Results are Owned or Abandoned only; a failed objective re-arms.
- Export: `challenges.json` gets a `tutorials` list (`challenges.py` `tutorial`): start and sign-up locators, map
  from the row's world (`dist_skateschool` = hash64 of the district), SpawnOffboard, IgnoreInitialTutorialReposition,
  intro / outro / restart presentation counts, and the objective rows the challenge row lists directly.
- In play (`crates/skate-game/src/tutorial_run.rs`, stepped from `challenges.rs`): started by a sign-up with the
  tutorial's id (or `SKATE_VERIFY_TUTORIAL=<id>` in verify runs); the objectives use the map's trigger volumes by
  name; Flow.ActivateObjective / DeactivateObjective / ResetObjective / GetObjectiveStatus and
  GetTotalNumCompletedObjectives are ported (changes applied after the message [inferred timing]), and the skater
  reads of b864 §2.2 / §2.3 in our state ids: IsInSequence (scoring), SequenceCompleted (a clean banked line this
  frame), IsWipingOut (state 300), IsOffBoard / WentOffBoard / WentOnBoard (states 500..502 and their frame edges),
  JustUsedSessionMarker / JustSetSessionMarker (Go To Marker / Place Marker as the group's marker messages), and
  SetFractionValue (kept for the HUD), and the trick reads of b863 (DidSpecificTrick / Group / ThisFrame variants,
  GetNumTricksDoneInTrickGroup): the scoring runtime records each announced trick's key (hash of its scoring_trick
  row; retail's trick-done message), the objectives keep them while active, and the trick groups are read from the
  stock collections (`challenge_trick_group`, a group's own list joined with its descendants'; checked: 87 groups,
  `fliptricks` reaches `kickflip`, `smithgrind` matches nothing as in retail); retail's
  `DidSpecificTrickgroup` typo stays unported. Also, from Ghidra reads of the binding tables (2026-11-06):
  IsInAir, IsOnBoard, IsOnGround, AirCompleted (states, the air-end edge and its group message), Input.GetRawRightStickYValue
  and IsLeft/RightTriggerHeld (threshold <= trigger, 82C63310 / 82C63430), Manual.Tail/Nose/ManualTime (the open
  manual carrier's seconds held), GetNumPushes (push contacts as the skater-pushed message, objective +0x4C0),
  GetPumpingAcceleration / IsLastPumpUserControlled (the ground pump controller) and Grind.IsGrindingGUID (the
  current rail's [spline id, type signature], kept by the grind provider; the argument is a decimal id). The start
  activates the first objective [inferred]. The result goes to the career. NOT RETAIL YET: presentation, the
  tutorial HUD and the end options are answered at once. A tutorial whose rows call an API function that is not
  ported does not run (logged with the missing names): an unported call errors and an erroring completion
  condition counts as complete in retail, so it would finish at once.
- Failure and hint rows (research b938, b940, b942): the export adds per objective `failures` (FailureObjectivesWithEvents)
  and `hints` (HintObjectives), each row its key, its condition program (none when it does not translate: never fires,
  retail's default false) and its presentation count, plus `failure_first` (EvaluateFailureBeforeCompletionObjectives).
  Per evaluation of an active objective (82C752A8): completion first; only when it did not complete, the hint rows
  (first true row plays its presentation, nothing else changes; on only when the group's hint flag is set, which
  MakeObjectives always sets) and then the failure rows (first true row: objectivefailed with the row, the objective
  stays active). With `failure_first`: hints, failures, then completion only when no failure fired. Both kinds share
  the objective's `vars` with its completion. A group whose failure or hint rows need an unported API drops that kind
  only (logged NOT RETAIL YET). Code: `objectives.rs` (`Conditions::failure` / `hint`, `Group::hints`,
  `Event::FailureCondition` / `Hint`), `challenge_objectives.rs` (`side_rows`). NOT RETAIL YET: the rows' tip
  presentations are logged (`SKATE_TUTORIAL <id> objective <i> failure row <r>` / `hint row <r>`), not shown.

## Change: install and refresh fixes (laptop runs 2026-11-06)

- `tools/install_prepared_hud.py` copied only the trick display, central message and minimap movies into the
  install, so the six challenge movies never arrived ("Challenge HUD ... not installed"). It now installs every
  optional movie `prepare_hud.MOVIES` exports (`OPTIONAL_MOVIES`, kept in step by a test).
- Refresh Game Files never rewrote `challenges.json` or `progression.json` on an existing install: they are written
  by the maps group's `post_maps` step, whose fingerprint did not include their exporters. `versions.py` now lists
  `challenges.py`, `objective_lua.py`, `progression.py` and `progression_lua.py` under `maps`.
- Trigger checks skip the ticks while the player's teleport is in flight (`trigger_volumes.rs`). Retail moves the
  entity within the frame; ours moves a tick or two later, so a challenge start entered and left the spot volumes from
  the sign-up point, about 45 m from the start locator (laptop point tests: the start locator is outside both).

## Change: own the lot export (`tools/asset_pipeline/challenges.py` `own_the_lots`; research b905, b916, b922)

`challenges.json` gains `own_the_lots`: every challenge_global_data row whose inherited ChallengeType is 0x34 and
whose child list (RefSpec array 0xABE749C80E8ADBCE; retail reads the key at bytes 8..15 and looks it up in
challenge_global_data, getter 8255B100) is not empty; DebugOnly lots (0xFD40E4A31027C984) are left out. Per lot: id,
source, map, district, title / description ids, the sign-up locator (Location, from the district's global locators)
and the children in disc order, each with its FreeskateActivityType (0x85D6B2C6AE61DD8B, inherited: 0 accumulation,
1 gap, 2 tag, 3 simultaneous trick, 4 survival, 5 group trick list) and title id. The engine sorts children by type
for the lot list (screen 0x1B); the key name does not tell the type (otl_indu_03_simul_01 is a gap). Disc export:
12 lots (10 career lots, otl_bbx_01, otl_mmcp_01), all with a sign-up location. Engine rules in skate-core
`challenges/lots.rs`: `owned` (every listed activity has a status) and `list` (stable sort by type, a header text id
before each type from `default_headers`, our list of table 8302CAB8's offline column, mod-replaceable; unknown types
show `#Invalid OTL Header`); tests `list_groups_by_type_keeping_disc_order`, `owned_needs_every_listed_activity`. Not
ported yet: the running lot, the lot list screen and MoreActivities.

## Change: the race export (`tools/asset_pipeline/challenges.py` `race`; research b978, b979)

`challenges.json` gains `races`: every bank row under the `races` family (ChallengeType 0x13), its race data read from
its local vault [data]. The chain (b979 §1): the params row (class 0x46A56ADDF284C381, key = hash64 of the challenge
key) `RaceHeats` -> `challenge_race_heats` rows (0xE933655B7553D1DE; the course build `sub_8255FDF0` looks the heat up
in that class [code]) -> `Legs` -> `challenge_race_legs` rows: `SplitTimeTriggers` (16-byte trigger instance items,
read like the teleporter triggers; the last one is the finish) and `Gates` -> `challenge_race_gates` (GateVolume,
IndicatorType; presentation only). Each row merges its parent chain inside its own vault. Per race: map, title /
description ids, radii, ChallengeType, `competitors` and `online_competitors` from the bank row (tCompetitorInfo: a
text offset into the bank .bin naming an `ai_characters` row, e.g. `reynolds_race`, and a kind: 0 that character, 1
a random pick from a pool, unused by races; research b987, all ten races resolve; `skater_profiles.json` now also
carries `ai_characters`: each racer key with its own AI profile (RefSpec 0x1540C7101EAEF5A6, e.g. `reynolds_race`)
and its look (RefSpec 0x4B60FF565334CCD6, e.g. `andrew_reynolds`), b993; a racer must use that profile, not its
look's `aiprofile`, which is `default` for most pros),
and per heat: the start locator (hull, else the district's global locators) with its `slots`: the locator record's
child matrices (+0x60 count, +0x64 table, stride 0x70; `teleports.py` `locator_slots`), 6 hand-placed starts on
every disc race. Retail places competitor n on child n (`82910EB0`; RepositionAI(id, StartLocation, id) in
setup_heat), the player is competitor 0 and AI racer k takes the k-th entry of the shuffled character list
(SpawnAIAtLocation 8284A248; research b989, b992). Slot 0 is not always the locator itself (9 to 22 m away on the
indu races), so the race start uses the slots, not the locator matrix. Then TimeLimit, TimeLimitWarning,
KilledItTime, AIPathIndexOverride and the AI path filter (tRoundPaths: path names, the all-paths flag at +0x320, the
value at +0x324), with each leg's EndTimeBonus, splits and gates as volumes from the hull.

Disc export (2026-11-07): 10 races (race_dwtn_01..03, race_indu_01..03, race_univ_01..04), each 1 heat and 1 leg;
split / gate counts, times, competitor counts and AI path lists match b979 §2, and every split and gate volume
resolves in its hull. Left out: the `_ol` online rows (as for the other families) and race_ind_01 (no hull, a
leftover). The engine does not read `races` yet; the tracker, AI racers and race machine follow (b978 port order).
Moddability: a mod can add a race from its own volumes and locators in the same shape; times, competitors and path
filters are data.

## Change: the race tracker (`crates/skate-core/src/challenges/race.rs`)

The retail race tracker (research b973, b975) [code]: per heat a course of legs, each a polyline from the leg start
(the race start, then the previous leg's last point) through the centers of its split trigger volumes (`sub_82560BC8`;
the volume point is the box center in world space, `sub_82DD8510`, which is our exported `shape.center`). Every tick
(`sub_82560170`) each racer's estimated time to finish = remaining distance / 9 m/s (the image constant 1/9 at
0x822F9118; no speed input, no smoothing; 0 once finished). The remaining distance (`sub_82560790`): from the racer's
split count g, the first point t > g whose projection along its incoming segment (`sub_82560A88`) is not behind the
racer, plus the polyline length from t to the leg end (`sub_825608B8`), plus the whole length of every later leg;
points past the leg end count 0 and a segment no longer than 1e-6 has no direction. Rank (`sub_8255FB38`): finished
racers first by their previous rank (finish order holds), then active racers by ETA, then disconnected ones; retail's
sort is unstable, ours keeps the join order on ties (our choice, so every peer ranks the same). Gap = ETA minus the
leader's, plus this racer's timer minus the leader's finish time once the leader finished.

Moddability: `RaceTuning` (reference speed, epsilon, the 1 s leaderboard refresh) defaults to the retail values and a
race can replace them; a course is built from any volume points. Multiplayer: the tracker is pure data keyed by stable
racer ids, deterministic and frame-rate independent; the authority runs it. Tests in `race.rs` (ETA at 9 m/s,
projection skip, later legs, zero-length segment, rank classes, gap). Not wired into the game yet.

## Change: the race run (`crates/skate-core/src/challenges/race_run.rs`)

The race challenge machine from the retail graph `race_shared` (research b876, b982), player first: opponents only
report their finishes (`OpponentFinished`). Flow: setup_heat (teleport to the start, tracker reset, leaderboard) ->
countdown (CountdownDelay 1.5 s, then the 3.0 s 3-2-1 HUD when Include321) -> run_heat (count-up race time; with a
time limit a hidden TimeLimit - TimeLimitWarning timer, then the HUD timer for the last TimeLimitWarning seconds) ->
the finish or time up -> killed_it (3.0 s EndRoundTimer, banner) -> cleanup_heat (best time, result banner Owned
3.1 / Killed 3.3 / lose 2.0 s) -> results (Continue completes, Retry runs the heat again). Pause restart re-runs the
heat; abandon completes as Abandoned. Splits count in leg order and may be skipped (any later split index), never
backwards; the split loop has no break, so a volume listed twice counts as its later index (b985); a leg's last split
moves to the next leg, the last leg's last split is the finish. At a leg end before the last, a heat with a time limit
and a leg EndTimeBonus above 0 rebuilds the heat timer as the time left plus the bonus (b985; 0 on every disc row;
what the bonus does while only the hidden warning timer runs is not read, ours then skips it). Races have no out of
bounds. When AI racers come: an AI's time-limit expiry also ends the heat in single player (b985 corrected b982).

Result: first place (PlaceHigherToWin, 1 on every row) in time owns it; also under the heat's KilledItTime kills it;
placing behind an AI skater or running out of time loses (Abandoned). This is how retail plays (USER-ASKS 516,
TrueTrophies Skate 3 walkthrough) and the graph's rule [data, b984]: in a normal race the finish does not set
PlayerWon (that write at `run_heat.triggerentercollision` pc 385-391 runs only for FollowMe races; Lua 5.1 `EQ 0`
then `JMP`); `end_heat` pc 213-244 sets it only when the local player is in places 1..PlaceHigherToWin of the
standings (score = racers - finish position) with a time above 0, and `killed_it` clears KilledIt without it. The
game mode test (`Challenge.GetGameMode` 828401D0) gives SinglePlayer in career, so this path runs. The best time is written on any finish, won or not, when there is no stored best (under
1 s) or the new time beats it [data, b982]. Our records keep `high_score: i32` (`challenge_records.rs`); races need a
time field when the run is wired in.

Moddability: every time, the place needed and the split volumes are data (`Params`, defaults the graph constants).
Multiplayer: the machine is pure events -> commands; the authority runs it. Tests in `race_run.rs`: killed it, owned
it, behind an AI loses but keeps the best time, the time limit's last 30 s then fail, skipped / backward splits, leg
change, best time and retry, abandon. Not wired into the game yet (AI racers, HUD, records).

## Change: races in play (`race_host.rs`)

First game wiring of a race, player only (b978 port step 4): `crates/skate-game/src/race_host.rs` reads `races`
from `challenges.json` and starts `SKATE_RACE=<id>` on that race's map (behind the Career and Challenges opt-in, like
`SKATE_CHALLENGE`). It arms the split trigger volumes, teleports the player to start slot 0 (b989 / b992), feeds the
player's trigger entries and 60 Hz ticks to the race run, updates the tracker from the player's position, shows the
challenge timer for the last TimeLimitWarning seconds and the owned it / killed it banners, writes ID_RACE_FINISHED
or ID_MISSION_CHALLENGE_FAILED as a central message, keeps the best time (`best_time` in `settings/challenges.json`,
a new optional field) and the completion, and sends the result to the career. Sign-up: see "Sign-up (2026-11-13)" under follow the leader. Not yet: DPO
lines, leaderboard, gates and the results screen (b995 / b997; results
continue at once, as the end dialog does without its movie). A completed race leaves the career's active list:
race_shared's `complete` calls CompleteChallenge with no re-activate flag (`CALL 5 3 1`, two arguments; research
b998), as films, follow the leader, hall of meat and contests do; only own the spot (true) and tutorials
(Persistent) pass one. Test: `race_host::tests` (a disc-shaped row loads, run parameters, slot 0,
course from the split centers).

### The 3-2-1-GO countdown (2026-11-08, b995 item 4)

- Retail [data, code]: after CountdownDelay (1.5 s) race_shared starts AddHUDCountdownTimer(3.0) and HUD2.StartCountdown
  (82850468: the `countdown` element visible, with the timer id) when Include321 is set. The movie `hud2/hudcountdown`
  (read from the disc movie 2026-11-08) has two states: SetState(2) = ShowCountdown, which reads
  `HUDComponents.Countdown_GetCountdownTime` ("#%d" of the seconds left, 8265D910), shows the number frame for "#3",
  "#2", "#1" and GO for anything else, and plays FE_SOUND_CHALLENGE_COUNT_3/2/1/GO (STAD_* when
  `Countdown_IsInStadium`); SetState(1) = HideCountdown.
- Ours: `RaceRun::countdown_hud_ticks` gives the HUD countdown's ticks while it runs; `race_host::countdown_hud` turns
  them into whole seconds (`ChallengeHudState::countdown`) and shows `Component::Countdown`; at the start it shows 0
  (GO) and hides after `Params::go_display_time`. `hud_challenge` sends SetState(2) on each new value and SetState(1) to
  hide; `Countdown_IsInStadium` answers false (races are not stadium events).
- NOT RETAIL YET: when retail hides hudcountdown after GO is not decoded; `go_display_time` = 1.0 s is ours (data a
  mod can change).
- ShowCountdown (disc movie, read 2026-11-08): `mcNumber.gotoAndStop(r)` on the frame labels "#3" (frame 0), "#2" (10),
  "#1" (20); for "#0" `mcNumber.gotoAndStop(FELanguage.GetLanguage())` on the language label ("English" 30, "Spanish"
  40, ...); then `mcCountdownText.gotoAndPlay("intro")` and the sound. Our movie runtime matched labels exactly, so
  "english" missed "English" and GO stayed on the 1 frame; labels now match without case, as AS2's do (`apt_movie.rs`,
  every movie).
- Laptop run H (2026-11-08): the count timing was right (shown 3 s, GO 1 s), but the screen showed the "1" picture for
  3 and 2 and nothing for 1, while the same movie from a fresh export draws 3, 2, 1, Go! (data-gated test below). Open:
  whether the laptop install's hudcountdown json and its texture files disagree (prepare_hud is incremental).
- Tests: `race_run::tests::the_hud_countdown_reads_its_ticks_only_while_it_runs`,
  `race_host::tests::the_countdown_hud_counts_three_two_one_go_then_hides`, data-gated
  `hud_challenge::tests::the_countdown_shows_each_number_frame` (SKATE_SCORING_HUD_ROOT=<private/hud>: number frames 0 /
  10 / 20, GO on a language frame, a different picture for each).

### The leaderboard (2026-11-08, b995 item 1, b997 section 4)

- Retail [data]: race_shared shows the LeaderboardHUD component (`hud2/leaderboardhud`) at setup_heat and hides it at
  cleanup_heat; the tracker pushes rank and gap (finish time once finished) every 30 ticks [code b973 §6].
- The movie (read from the disc movie 2026-11-08 with `.local/tools/apt_dump.py`): SetState 1 = Show(true), 2 =
  Show(false), 3 = SetupScreen, 4 / 5 = RefreshAll, 6 = UpdateVOIPIcons. SetupScreen reads
  `HUDComponents.Leaderboard_GetSetupDataBlock` [players, top-team players, layout, (unread), is teams, online mission,
  show players] and `Leaderboard_GetLeaderboardNameBlock` (names in data order). RefreshAll reads
  `Leaderboard_GetLeaderboardDataBlock`, `playerStructItems` (8, set in the movie) items per row: rank (drawn "#" +
  rank), score, score text, highlight, turn, team, row slot (the `player_<n>` clip), sort id; `SetPlayerScore` shows
  the score text when the score is 0 or more, else "--". The movie sets its own constants (eLeaderBoardHUD_RankNameTime
  4, MAX_PLAYERS_AVAILABLE 8). `FELeaderboard.GetVoipStatus` is only read in online missions; `Mission.GetTitle` gives
  the title label.
- Ours: `hud_challenge::Component::Leaderboard` (own sync: 3, 1, 4 to show, 4 on a change, 2 to hide),
  `ChallengeHudState::leaderboard` (`LeaderboardHud { layout, title, rows }`, `LeaderboardRow { id, rank, score,
  score_text, name, highlight }`, serialisable, stable racer ids), the natives above; `race_host` builds the rows at
  `Leaderboard(true)` (the player as a `#name` literal from settings/player.json, each AI racer by its look's `name_id`
  from skater_profiles.json, e.g. ID_COMMON_DANNY_WAY) and refreshes rank and time from `RaceTracker` every
  `RaceTuning::hud_refresh_seconds`. Export: `tools/prepare_hud.py` adds `leaderboardhud.json` and
  `tools/install_prepared_hud.py` installs it (a Refresh Game Files brings it to an install).
- The DPO sits above the leaderboard: the leaderboard movie writes `_global.LeaderboardHUD_Height` and the DPO movie's
  SetupDPO (run on its SetState 1) moves up by it when it is over 163 [data, both disc movies]. Retail runs all HUD
  movies on one `_global`; ours run each on its own VM, so `hud_challenge::SHARED_GLOBALS` names the value and its
  writer, the writer movies update first and the value is copied to the others before each movie updates.
- Setup block item 3 is the variant line under the title (HUD2 SetVariantString; hidden for "#Unknown" /
  ID_COMMON_UNKNOWN or a throwdown) [data movie, code b882 8265DBC8]. `LeaderboardHud::variant`, default
  `LEADERBOARD_NO_VARIANT` (hidden).
- Solid units (draws with no texture, e.g. the row backers) sample a plain white texture in the HUD renderer, and a draw
  whose texture is not loaded takes no slot (race run K panicked indexing past the slots).
- NOT RETAIL YET / open: the row time text (8265E510's sprintf) is not decoded, ours is the DPO time text (M:SS.hh) of
  the gap or finish time; the title (Mission.GetTitle) is empty, the race's title id is not in our export; which
  variant string races set (if any) is not traced, ours hides the line; the row slot is rank - 1.
- Row times (retail as the user describes it, 2026-11-09): the leader's row shows the running race time, every other
  racer still riding its gap behind the leader, a finished racer its finish time (`race_host::row_time`). Before, the
  leader showed its own gap, 0:00.00. Gaps carry a leading `+` [code].
- Row text [code, research `.local/research/autopilot-open-questions.md` Q2, recomp checked]: the data block is
  `8265DFD8` (`8265E510` is the name block, not the time); a finished row (row byte +0x3B) shows its time
  (`sub_8265DDF8` flag 0), an unfinished rank-1 row copies a fixed string (read as "--" at 0x821F4B5C, not the race
  time), other rows their gap with a forced `+` sign (flag 1); a value of about 0 reads `ID_RACE_LEADER`. Ours follows
  the user's account for the leader (the running race time); open: which layout races set and what the leader string
  is in a real race.
- The Killed It banner reads "KILLED" [data]: `killedit` has three text fields, all `ID_CELEBRATE_KILLED` = "KILLED"
  in the disc's string table, and none of its nine bitmaps holds "IT" (checked 2026-11-09).
- Test: data-gated `hud_challenge::tests::the_leaderboard_shows_ranks_names_and_times`.

### Minimap race gates (2026-11-09; research `.local/research/race-gate-indicators.md`, [code] 82849B78 / 82849C88 / 8254E408, [graph] race_shared)

- Retail: at the countdown every gate of the leg goes on the minimap (`Map.AddRaceGate`, fe_map_icons `race_gate` =
  `mini_gate`) and the finish (`Map.AddRaceCheckPoint`, `race_spotgate` = `mini_spotracegate`), all in MiniMapManager
  group C (+0x438), at the volume's world (x, z), angle 0, scale 1, no tint; after each gate hit only the next
  `MaxMiniMapIcons` (3) gates show, which is also the group's draw cap. Every career race is skipable, so there is no
  "next full / after-next gray" styling (that branch is for non-skipable races; corrects b876 / b995).
- Ours: the export's per-leg `gates` (in course order) join the race's trigger volumes; the race host counts the
  furthest gate entered (`gates_passed`, a skipped gate counts as passed) and keeps a `race` minimap group of the
  window `skate_core::challenges::race::visible_gates` (the next `GateIcons::max_icons` gates; the finish once the
  window has room, ours for the nearest-three cap); the group goes at `RemoveGates` and at the race end. Icon records and
  `max_icons` are data (`GateIcons`, retail defaults) and read through the same fe_map_icons lookup as mod icons.
- Fix found on the way: the minimap loaded only the needle and `mini_skater`, so any other icon (race gates, mod
  icons naming another record) drew nothing; it now loads every `mini_*` texture of the menu movie set (31).
- Verified on the desktop: race_dwtn_01, `RACE_MINIMAP gates passed 0 of 13: 3 icons`, then 1 of 13; the screenshot
  shows the gate icons on the minimap rim (the next gates are beyond its 71 m radius).
- Open: whether 82DD8510 gives the volume's origin or box center (ours: the box center); the ribbons (anchor native
  `AddTriggerIndicator` not decoded yet).

### Race gate ribbons (2026-11-09; research `.local/research/race-gate-ribbon-anchor.md`, [code] 82D50418 / 827A9F48 / 827AF330, [graph] race_shared)

- Retail: at the countdown race_shared calls `VisualIndicators.AddTriggerIndicator(group, gate volume, IndicatorType,
  RibbonInstance)` (82D50418) for every gate of the leg, then for the finish (the last split) with
  `RaceFinalGateRibbon` (red, one leg) or `RaceLegFinishGateRibbon` (green). Entries are keyed by volume id, so a finish
  listed as a gate replaces the gate's ribbon (last add wins). The ribbon sits in the box frame (center, axes) and is
  scaled per axis to the box (2 x half extent / spline size; a flat spline takes the x scale for z, `827AF330`), with no
  indicator offset; the indicator row's Enabled flag is not read; its style is the row's own (`default`). Gates are
  type 2 (RaceNextGate). Passed gates keep their ribbon (it is only removed for gates with a Time_Bonus, 0 on every
  disc gate); everything goes at cleanup / retry (DeleteGroup).
- Ours: the export adds per gate `ribbon {indicator, colour}` (RibbonInstance, two RefSpecs), `time_bonus`, `audio`,
  and per race `gates {skipable, max_minimap_icons, grey_icon, grey_ribbon, final_ribbon, leg_finish_ribbon}` (null
  when the race vault leaves it to the framework; the game uses retail defaults). `race_host::gate_ribbons` fills
  `visual_indicators::RaceIndicators` at ShowGates (every gate of the leg, then the finish replacing a gate on its
  volume) and clears it at RemoveGates / the race end; `Placement { always, fit }` draws a disabled row and fits the
  spline to the box (`fit_scale`); `Hash_` row names resolve through the attrib hash.
- Open: whether the strip builder (827B0BA8) recentres the spline (the ribbon height on the box); what reads flag bits
  29 / 30 (next gate / finish may look different in retail).

### Race results table (2026-11-09; research `.local/research/race-results-rows.md`, b925; [code] 8265E950 / 828534B0, [graph] build_results / end_heat)

- Retail: build_results writes every competitor (finish order from end_heat's RoundStandings; an unfinished AI gets
  the count-up time plus Race.GetEstimatedTimeToFinish, an unfinished player DNF), title
  `ID_CHALLENGE_COMMON_RACE_TIMES`, layout eResultsLBType DeathRace (3; b982 / b995 said PlaceNameScoreTime 6, wrong),
  columns rank / name / time, status PASS / WON (within PlaceHigherToWin) / LOSE; slots Continue_Retry, Retry_Quit,
  ChangeDifficulty after a loss or exit. The movie: a lost race shows challengefailed_small (no table), a won race with
  2+ rows endchallengeoptions (the table).
- Ours: skate-core `challenges::results` (`race_results`, `ResultsView`, the 8-item data block, `ResultsRules` with the
  retail ids as data); race_host builds it at `Results`, opens the end dialog (`SKATE_RETAIL_END_SCREENS=1`) and turns
  the player's choice into Continue / Retry; with the autopilot driving it continues after 3 s. end_dialog_host answers
  Results_GetNumPlayers / LayoutType / LeaderboardDataBlock / SelfRank / LeaderboardColTitle / ThrowdownType /
  LeaderboardType. The table movie reads `endchallengeoptions.STRUCT_ITEMS` and `eResultsLBType_*` that retail's
  native code sets on the class: the stage sets them before ShowScreen (the first laptop run showed an empty table).
- Verified: lost races show "Didn't cut it." with Quit / Retry / Change Difficulty (laptop race-results, 7 runs); the
  table after a win is NOT verified yet (statics fix pending a laptop rerun).
- Empty table after a win (laptop race-results5 and the stage test `retail_end_dialog_results_table_builds_its_rows`,
  2026-11-09): the statics arrive in time, but the table control (`source/controls/table`) builds each row as a
  `createEmptyMovieClip` clip (`row_N`) and attaches its cells (`col_N`) into it; our empty clips were script objects
  outside the movie's display tree, so `attachMovie` into them was unhandled and nothing drew. Fix: such a clip joins
  its movie's display tree as an empty sprite on the first `attachMovie` into it, with any empty clips between it and
  the movie (nested empty clips; which one the last unhandled call hit is not confirmed) (`Movie::adopt_empty`, standard AS2,
  NOT RETAIL YET); holder clips that only get a movie loaded into them (main, screenmanager, main3dhud) are unchanged. Verified by the laptop (results-table-test3, 2026-11-09 19:15): the stage test builds 4 rows with rank / name / time cells; apt_ 46 and retail_ 126 tests pass. In game: race-results6 had no win in 12 runs; `SKATE_RACE_PLACE_TO_WIN=<n>` (test switch, `race_host::place_to_win`, retail 1) lets any place up to n win so the won dialog can be checked. Seen in game (laptop race-results7, 2026-11-09 20:11, race_indu_01 with the switch): the title, the Rank / Name / Time headers, one row per racer in finish order with M:SS.hh times, the player's row highlighted, A Continue / B Retry. Fixed after it: the player's name showed `#Player` (an already literal cell got a second `#`). Fixed after it: the Rank header started outside the panel (next section). Open: a 23 to 30 ms hitch when the dialog opens. The switch only changes the dialog's pass (`race_results` + `passed`), not the race graph's result: a forced win ends `completion=Abandoned` with no pay and no reward screen. Laptop event-args-probe (20:40, a real win, race_indu_01 80.15 s): the player's row reads `Player` (fixed). Open: Darren Navarrette, still riding (no finish line for him in the log), showed the player's time 1:20.15 = the frozen race time + an estimate of 0, while Lizard King's 1:31.60 is the same time + his estimate. An estimate stays 0 when the racer's position is never found (no spawned NPC) or his remaining distance measures 0; the results now log every racer's leg, split, estimate, finish and whether its NPC exists (`SKATE_RACE <race> standing racer ...`) to tell which. Laptop results-headers (2026-11-10 00:40, 3 runs, all the same): Navarrette `leg 0 split 13 eta 0.00 finish None npc true`, so he is found and has passed split 13 of 14; the estimate measures to the finish point (`Course::remaining`, sub_82560790) and is 0 once he is past that point's plane. He is past the finish point but was never inside the finish volume (the AI tests one point, the deck position, per update). The AI now logs once `SKATE_RACE racer <id> past the finish point outside its volume: at <pos>, volume <box>` to show how he misses it. Same runs: the dialog-open hitch (28 ms) names `slowest=animation`. Laptop finish-volume (2026-11-10 00:03): the player lost this run (89.85, 3rd); Lizard King (74.58) and Navarrette (82.98) both finished through their finish volumes and no `past the finish point` line was logged, so the case did not come up; the line stays in for the next won run. Pause menu (verify path) unchanged after the header-width change.

### Save tag (2026-11-09; b948 / b957 S4)

- The autosave layer's corner tag (`_global.Save_EnableTag("false", "false")` / `Save_HideTag`, 825DF4F8): shown while
  any background save write runs (career, challenge records, career stats hold a `save_tag::Writing` guard), hidden
  when none does and it has been up 3.0 s (`SaveTag::min_seconds`, data); hiding re-shows the load tag during a load.
  When saves happen is unchanged (b957 S1-S3 open). Verified on the desktop: shown / hidden 3.0 s apart at a race end.

### Presentation runtime core (2026-11-09; b964 §2 R4-R11, port step 2)

- skate-core `presentation`: `Manager` with retail's queue (start now when idle, else queue in 8 slots; skip a group
  equal to the playing one with an empty queue or to the last queued; drop when full), events in order with mask skips,
  a finish message per group, skip = stop the running event + undo the run ones in reverse, `remove_owner`. Events are
  the career's `graph::Event` (type by name); the game's `Handlers` come next (registry, tutorial / career wiring).

### Presentation handler registry (2026-11-09; b964 §3, port step 3)

- skate-game `presentation_run::Registry` implements the manager's `Handlers` by type name: all 73 retail types
  (skate-core `presentation::EVENT_TYPES`, the export's order, b874) have a handler, so no group stalls on an unported
  type. `Wait` (type 4, 8257C820) is ported: arg1 seconds of game time [code b900]. The rest are logged stubs, done at
  once (`PRESENTATION stub <type>`); NOT RETAIL YET for the waiting types (NIS, FMV, Speech, Teleport, Fade, which retail
  holds until their end message). An unknown name logs and finishes.
- Moddability: `set_mod(mod, type, handler)` adds a type or overrides a retail one while the mod is on; `remove_mod`
  drops them and the engine handler is back (run `Manager::remove_owner` first so the mod's running events stop).
  Ported hosts replace stubs with `set_engine`.
- Wired to the tutorials (step 4, next section); the career is next. Tests: `presentation_run` (3: every type
  handled and unknown ones finish, Wait holds its seconds, a mod override and its removal).

### Presentation event export (2026-11-09; b964 §5 step 1, b943)

- `challenges.py` now writes the events themselves, not their count: tutorial `presentation.intro / outro / restart`
  (the row's own fields, args from its local data binary) and every failure / hint row's `presentation` (args from the
  row's own bank binary), each `{type, args: [arg1, arg2], mask}` through `progression.presentation_events` (b892 / b874
  type table). The engine reads both forms (`challenge_objectives::EventList`: a list, or a count from an older install),
  so tutorials keep loading before a Refresh Game Files. Expected rows: b943's per-row table (masks all 0). Played by
  the presentation manager since step 4 (next section).
- `decoded_events` reads the arguments from the row's binaries in turn (tutorial: local vault, then bank; failure /
  hint row: its own bank binary, then the tutorial's) and keeps the count with a `challenges:` stderr line when none
  fits. The first version raised instead and the export skipped the tutorial (laptop presentation-export 2026-11-09:
  1 of 12 tutorials left). Laptop presentation-export2 (2026-11-09 20:24): 12 tutorials / 94 objectives / 16
  challenges back; every failure / hint row decodes (tut_sksc_08's rows match b943; b943's sixth row tut_sksc_08_3 is
  not in tut_sksc_08, a b943 table error); all 599 masks 0; SpeechNoWait's id arrives as the string "5360". Open: the 22
  tutorial intro / restart lists decode from neither the local vault nor the bank binary and stay counts;
  `SKATE_EXPORT_EVENT_DEBUG=1` dumps their records and the bytes at each offset. Laptop event-args-probe (20:41): the
  arguments are in the tutorial's local vault binary (`tut_sksc_01`, `tut_sksc_01_startlocator`, `8191`); the first
  string follows the .bin's 8-byte header, so `progression._cstr` now also accepts offset 8 (`BIN_DATA_START`).
  Verified (laptop presentation-export3, 2026-11-09 20:54): 0 counts left, 314 lists, 658 masks all 0, unittests 14 OK; numeric arguments stay strings ("8191", "5360"; retail atoi / atof them in the handlers).

### Results table header widths (2026-11-09)

- Symptom (laptop event-args-probe, race_indu_01): the Rank header started outside the panel and the Time header sat
  left of its column; Name was right.
- Cause [code: source/controls/table, the header loop after SetColHeaders]: each header cell is a `Header Text` clip
  `col_N` holding `headerText` with `autoSize = "left"`; the table places it by justify with the CLIP's `_width`:
  right `colXLoc + (colWidthMax - _width) - 2`, center `colXLoc + (colWidthMax - _width) / 2`, left `colXLoc`. Our
  clip `_width` was the static frame-0 bounds (the authored field box, 104), not the autosized text, so right / center
  headers moved left by the difference (Rank -33.9 instead of 0, Time 434 instead of 504).
- Change (`apt_movie.rs`): when an autosized field is measured, the sprites holding it get `_width` / `_height` from
  their live children (`live_bounds`: the bounds walk 82E59C28 over the display list, clip-depth children left out;
  an autosized field counts with its measured width anchored at its autoSize side), times their scale.
- Verification: the data-gated `retail_end_dialog_results_table_builds_its_rows` now checks each header clip's `_width`
  equals its text's (fails without the change: 104 vs 33.6 to 43.5); apt_ 46 and retail_ 126 pass.

### Tutorial presentation (2026-11-09; b964 §5 step 4)

- One manager per game (`presentation_run::Presenter` in `Challenges`, retail's PresentationManager singleton). A
  tutorial's StartPresentation (intro / outro / restart) adds the exported list as group `<tutorial>/<slot>`, owner the
  tutorial id; the state machine gets PresentationFinished when the manager reports that group finished, so a Wait in
  the intro now holds the teleport. A list with no events (an export with counts only) or a dropped group (queue full)
  finishes at once.
- A failure row queues its tip group `<tutorial>/obj<N>/failure<R>` before ObjectiveFailed reaches the state machine
  (so the restart group queues behind it); a hint row queues `.../hint<R>` only (82C755B8, b942).
- The manager runs once per frame on the frame's game time (the tutorial's 60 Hz ticks), plus zero-time passes so an
  instant group finishes in the frame it starts. Completion and abandon remove the tutorial's groups (82DBA8F8): the
  running event stops, queued ones go unplayed.
- NOT RETAIL YET: every type but Wait, FadeIn and FadeOut is still a logged stub done at once (callouts, NIS,
  teleports); the manager runs only while a tutorial runs. Tests: `tutorial_run` (the intro holds the teleport for its Wait, abandon removes
  the tutorial's groups; the existing run test with count-only lists).

<a id="presentation-fade"></a>

### Presentation fade (2026-11-10; b900 §1, b902, b964 §5 step 5)

- Retail [code b900 / b902]: FadeIn / FadeOut events (8257B040) start a slot of the owner's fade object (+0x214;
  82D5BEB0) with arg1 seconds (lenient float 82AE9028). The object's update (82D5BB80) takes a fixed 1/60 s off each
  active slot per tick and posts the overlay alpha: FadeIn remaining / total (1 to 0), FadeOut elapsed / total (0 to
  1), flash a 0 to 1 to 0 triangle split at 0.5. A FadeOut that reaches 0 holds black (alpha 1) and posts
  cMsgEffectFinished(2); a FadeOut while held finishes at once; a FadeIn clears a held FadeOut. Other modes count 5
  ticks after 0, cancel (overlay off), and post their finish on the next tick. The event is done on the finish
  message for its own mode; undo cancels every slot (82D5C010); stop does nothing.
- Ours: `skate_core::presentation_fade::Fade` (slots, `start`, `tick`, `cancel_all`, `reset`; `FadeParams` step 1/60,
  end ticks 5, flash split 0.5 as mod-replaceable defaults). `presentation_run::Presenter` owns one shared
  `FadeHost` (whole 60 Hz ticks from the frame time, last alpha, finishes seen); `FadeHandler` registers for FadeIn and
  FadeOut; `Presenter::update` steps the fade, then the manager; `Presenter::fade_alpha` gives the alpha.
- NOT RETAIL YET: the alpha is not drawn. Who draws it and whether it reads as black is open (b904: the subscriber is
  a post_fx color matrix blend whose dumped target is a warm tint). Retail's tick cadence (per 60 Hz tick or per
  frame) is open; ours is frame-rate independent at 60 Hz.
- Tests: `presentation_fade` (FadeOut linear and held, second FadeOut at once, FadeIn clears held; FadeIn finishes
  six ticks after 0; flash; cancel and reset), `presentation_run` (a FadeOut group finishes when black, a FadeIn
  clears it, the lenient float).
- Laptop presentation-fade (2026-11-10 02:12): build OK, presentation_fade 4, presentation 4, tutorial 2 pass. The
  exported tutorial lists have no FadeIn / FadeOut event (most used: ShowTutorialCalloutHUD 172, SpeechNoWait 138,
  HideTutorialCalloutHUD 53, SFX 52, Wait 48, NIS 22, SetSimRate 22, TeleportLocalPlayer 19, RestoreSimRate 19), so
  no fade played in game; tut_sksc_01 with pass unchanged (Owned, no group stuck).

<a id="tutorial-hud"></a>

### Tutorial HUD (2026-11-10)

- Retail [data b858]: make_hud creates the tutorial's HUD group {ObjectiveHUD, TutorialInstructionHUD, OwnedItHUD};
  challenge_running shows ObjectiveHUD, the owned it banner shows at completion.
- Ours: the tutorial's ShowHud commands (only logged before; the user saw no tutorial HUD in the desktop run) are now
  `tutorial_run::Action::ShowHud`; `challenges.rs` shows / hides `Component::OwnedIt` from them. Tests:
  `tutorial_run` (objective HUD requested on arrival, owned it banner at the end).
- NOT RETAIL YET / open: the objective HUD stays hidden for tutorials: its row texts (Objective_GetObjectiveTitle,
  objective data +0xd0) are not known. The objective records carry only key, activation mode and definition (the
  definition's second u32 is 0, not a description: checked on the disc 2026-11-10); own the spot sets its row with
  Objectives.ChangeObjectiveText, so a tutorial's texts come from Objectives.MakeObjectives / its HUD hookup (to read).

<a id="presentation-speech"></a>

### Presentation speech (2026-11-10; b874 §2 / §3, b1014)

- Retail [code b874]: Speech (2) / SpeechNoWait (3) start 8257C308 map arg1 (speaker name) to an eSk8Characters id
  through a map built from the 104 names (TU3 0x82232694; SkateCoach 31, Reda 30, CameraMan 40), take arg2 as the
  speech id and call the speech system vt+0x3C(character, id, 0). Speech holds its group until cMsgSpeechEnded
  (payload +0x10 character, +0x14 id; posted when the stream ends, or at once when the line is not found, b1014
  status) or a 10.0 s timer (821963E4); SpeechNoWait finishes at once. Ids from 5000 take the NIS path (b1014).
- Data [data]: `data/audio/<lang>/nis.big` holds 927 bare `.sns` bodies `<char>_<id>_<CAT>-<TAG>_<take>[_360]`
  and `nisresident.big` one 8-byte `.snr` header each (532 (character, id) pairs; up to 12 files per pair).
- Ours: setup extracts the speaker names into `menu-tables.json` `speakers` (`skate_data::menu_tables::speakers`,
  checked on the TU3 image and the disc build: 104, same order; `menu_tables.py` requires them) and exports
  `private/audio/speech/nis.json` (926 lines: character, id, category, tag, take, x360, snr) with every line
  decoded to `speech/nis/<file>.wav` (`world_audio.nis_index` / `decode_nis`; mono 48 kHz, checked on three
  lines; not gated by the Ped Speech switch). `presentation_run::SpeechHandler` resolves the speaker and line,
  queues a `SpeechPlay` (`Presenter::speech_out`) and, for Speech, holds the group for the line's length capped at
  10 s; an unknown speaker or a missing line finishes at once (logged).
- Takes [code b1014, verified on the disc]: retail builds each line's take list from nisresident.big's file table
  in order (824A5A08: character = name chars 0..1, id = chars 3..6; 927 files, 626 lines: 490 with 1 take, 59 with
  2, ... one with 12; nis.big's order is the same). 82482130 plays them round-robin; at the start of each pass a line
  with more than 2 takes is shuffled with count / 2 random swaps (824821C0), so 2 takes alternate. `_360` takes are
  never preferred (no take has both). Ours: the export walks the resident table the same way (927 lines);
  `SpeechLines::pick` keeps the order and cursor per line, with a seeded xorshift in place of the global PRNG
  (82A8AF10). One stream: a request while a line plays does not play and ends at once (824A5568 posts Started +
  Ended); ours tracks `SpeechLines::playing`.
- Retail mix [inferred b1014]: 2D on the main cast's stream channel 0 (no actor or position); MixMap Obj:Speech input
  2 is raised while the scripted-dialogue state (+0x1BD28) is 2, which ducks the music. Subtitles: each take shows
  string `SPEECH_SCRIPTED_<file>` (our HUD export already holds 884 of them) through message 0x61363201.
- Playback: `game_audio::nis_speech` takes the picked lines (`Presenter::speech_out`), reads the wav on a thread
  (`nis-speech`), puts it in its own mixer bank (`skate_audio::world::speech::NIS_BANK`, beside the living world /
  main cast / announcer speech banks) and opens a 2D direct voice (gain 1, no position); while it plays,
  `WorldSpeech::scripted` raises MixMap Obj:Speech input 2 (the retail Global then ducks the music). Logged
  `AUDIO_NIS_SPEECH <file> voice ..`.
- Laptop presentation-speech (2026-11-10 12:01): build OK; Refresh Game Files 404 s (environment + audio redone):
  `speech/nis` 927 wav (732 MB), menu-tables speakers 104; menu_tables on the disc data 2, presentation 8, tutorial
  2, setup 12 pass. Startup `104 speakers, 626 lines`. tut_sksc_06: SpeechNoWait SkateCoach 5227 picks
  31_5227_TUT-JORD_1 (2.13 s), the mixer opens its voice 9 ms later; the second line 5213 4 ms later is refused
  (one at a time). tut_sksc_01: 5012 -> 31_5012_TUT-JORD_3 (5.08 s) plays. Runs muted: not judged by ear yet.
- Desktop run (2026-11-10 12:25, after Refresh Game Files, user watching, sound on): tut_sksc_01's SpeechNoWait
  SkateCoach 5012 played 31_5012_TUT-JORD_3 0.8 s after the tutorial started; the user heard the line clearly
  ("You'll only need a couple of pushes to get up to speed. Coach Frank uses one." in the retail subtitle string);
  the callout followed 0.2 s later. No music played (the engine has no licensed-music player), so the ducking was
  not heard. The tutorial's objective HUD did not show: the ShowHud commands are not wired yet (separate item).
- NOT RETAIL YET / open: the line's gain and the main cast channel's stream DSP (ours: a plain direct voice);
  subtitles; the `Unknown` speaker's AudioCharacterOverride; Coach Frank's quit line
  (31 / 5706) queued after the current line; the per-character flag (2 / 3 / 0) passed with the line; other
  languages are not exported.
- Tests: `presentation_run` (Speech plays and waits for the line, SpeechNoWait and an unknown speaker finish at
  once; takes round-robin, 2 alternate, more shuffle per pass); `skate-data` menu_tables (speakers on both builds); setup `test_world_audio` (line names),
  `test_menu_tables` (speakers required).

<a id="career-presentation-groups"></a>

### Career presentation groups (2026-11-10; b964 §5 step 7, R12)

- Retail [code, Ghidra 2026-11-10]: Events.ExecuteEventsAndChangeState (cfunc 827D16F0; binding name 821A7D18,
  table 82311734) hashes the target state name (FNV-1a 64), builds a callback object (vtable 82311908) and asks
  827D1480 for a group of the current state's events. No events: the state changes at once. Otherwise the group
  goes to the presentation manager (82DBACA0: start now or queue, up to 8) and the change happens in the callback
  when the group finishes (827D1420: graph vt+4 with the hash). With its second argument true and a global flag set
  it first calls 827CFC58 (clears pause reason 6 and some HUD bits) [code; purpose inferred]. DeferEventsAndChangeState
  (827D1970) builds the group with 827D1870 (callback 82311900), pushes it on a deferred list without starting it,
  and changes state at once.
- FIXED: ours changed state in the same call for Execute. Now `graph::Runner` holds the target (`Running::waiting`)
  when the state has events and `Runner::events_finished` / `Career::events_finished` enters it; `Request::Events`
  carries the target.
- One presenter for the game: `presentation_run::Presenter` is a resource (retail's singleton), updated once a frame
  by `presentation_run::tick` (unless a tutorial already ran it with its zero-time passes), with a finished-group
  list (`take_finished`). The career host (`career.rs` `run_presentation`) starts an Execute group
  `career/<graph>/<state>` (owner `career`) and calls `events_finished` when it finishes (at once if the queue
  dropped it); released deferred events start as `career/deferred/<n>` groups; GrantAchievement is a handler
  (`GrantHandler`) whose names the career grants. Without a presenter the old at-once path remains.
- Tests: `career::graph` (Execute waits for the state's events, a state without events changes at once).
- Laptop career-presentation (2026-11-10 11:27): build OK; core career 40, game career 5, presentation 6, tutorial 2,
  challenge 21 pass. Progression: 247 graphs, ChangeState none 244 / Execute 17 / Defer 27; 10 land on a state with
  events (Execute: press_photo_anyw_11, gpmm_starting, gpmm_billboard_photo, photo_anyw_01 / 11 / 21 / 31 / 41
  billboard celebrations; Defer: tut_skpk_univ_01 meet_photographers, ftl_univ_03). The tut_sksc_01 pass run starts
  none of them, so the wait is not seen in game yet (needs a photo billboard or tut_skpk_univ_01); it ends Owned,
  the callout hides with -20 at completion, save restored identical.
- Open: the 827CFC58 flag; the career's callout / sim-rate states only reach the HUD while a tutorial runs
  (`take_callout` and the rate posts sit in the tutorial branch).

<a id="trick-done-and-began"></a>

### Trick done and began lists (2026-11-10; b863 §2, b865 §1 / §2)

- Retail [code b865]: the trick-done message 0x92CC9BBD (which the objectives' trick list and DidSpecificTrick /
  DidSpecificTrickGroup / GetNumTricksDoneInTrickGroup read) and the ThisFrame variants come from the ended list,
  filled only when a trick carrier completes landed (82DA5F98), and only for a trick that was announced first
  (rec+0xBE clear); bailed or unannounced tricks post nothing. BeganTrick / BeganTrickGroup (82C670D8 / 82C671D8)
  read the began list, filled on the announce edge (82DA5DE0).
- FIXED: ours fed the announce edge to the done reads, so a trick counted as done the moment it was announced, even
  when the skater then bailed. Now `ScoringRuntime` keeps both: `announced` (the began list, `take_announced`) and
  `done` (a valid carrier that completes landed and was announced, `take_done`); the tutorial passes `done` as its
  trick messages and ThisFrame list and `began` to the new BeganTrick / BeganTrickGroup (exact key, or any member of
  the group).
- Effect: tut_sksc_06's hint rows (Tricks.BeganTrick) now load; rows that ask for a trick count it when it lands,
  not when it is announced.
- Laptop trick-began (2026-11-10 09:31): build OK; challenge_objectives 1, tutorial 2, scoring 8, presentation 6
  pass. tut_sksc_06 loads 5 rows, 0 failure rows, 31 hint rows (was 0 hint rows); hint rows 2 and 4 fire at the
  start: callout 246 + 10 (key bs_5_o, ID_CHALLENGE_TUT_SKSC_06_HINT_03), then 10 (popshuvit,
  ID_TRICK_FLIP_POP_SHUVIT). BUG: the verify shot at 41 s shows no callout panel; gesture tips (10) draw nothing in
  game, as in the data-gated test, while text tips (12) draw.
- FIXED (2026-11-10): the generic HUD show / hide are SetState -10 / -20, not 246 / 236. The movie pushes them with
  the signed-byte action 0xB5 (our VM decodes it signed, `apt_vm.rs` 0xb5); b866's table read the unsigned dump, and
  b945 already noted 826678A0 sending -10. So our show never matched: text tips drew only because state 12 sets
  `mScreen._visible` itself; a gesture tip (10) stayed invisible. Found with a per-state trace in the data-gated test
  (show 246: mScreen stays hidden; 12: visible, 11 to 12 drawn units; 10: hidden, 0 units). Now
  `callout::STATE_SHOW` = -10, `STATE_HIDE` = -20, and the test asserts the gesture tip draws the menu set's
  gesture_item textures (desktop: passes).
- Laptop callout-show-fix (2026-11-10 10:18): build OK; presentation 6, tutorial 2, challenge_objectives 1,
  scoring_runtime 14 (data-gated included), the callout test pass. tut_sksc_06: -10 + 10 (bs_5_o), then 10
  (popshuvit); the shots show the panel with the right-stick gesture icon (RS, arrow) and "Pop Shuvit". tut_sksc_05:
  4 rows, 0 failure rows, 32 hint rows (was 0); its first tip (text, HINT_03) shows -10 + 12 and hides 10 s later.
- ObjectiveAir.GetTimeToGrind and IsDoingSpecificBoardAdjust (b1013, 2026-11-10) [code]: GetTimeToGrind (82C65E70)
  reads the air output: while +0x1BB is set (82DB6EC0 copies trajectory selector byte 9653, set when a grind is
  found on the center trajectory) it returns +0xB4 - +0xB0 (predicted contact time minus time in the air, seconds),
  else FLT_MAX (DAT_8206D108). Ours: `AirOutputFields` `flag_443`, `collision_time_180`, `time_in_state_176`
  (`skater.player_input.physical.air`), `api::NO_GRIND_TIME`. IsDoingSpecificBoardAdjust(n) (82C65FD0) compares n
  with the board adjust 82BE1AE8 copies from the animation packet (1 left, 2 right, 3 up, 4 down): ours
  `ScalarAttributeInputs::board_adjust` (`skater.animation_input.fields.board_adjust`), `api::BOARD_ADJUST_*`.
  Both now in `SkaterFacts` (its default keeps FLT_MAX). tut_sksc_05 uses `GetTimeToGrind() <= 0.5` and adjusts 3 / 4.
  Not proven (b1013): that 82C73E50's record is 82DB6EC0's player block; when our selector reset clears flag_443
  against retail's 82D67228.

<a id="presentation-sim-rate"></a>

### Presentation sim rate (2026-11-10; b900 §5, b902 §2)

- Retail [code b900 / b902]: SetSimRate (47) calls the sim controller's vt+0x7C with 1.0 / atof(arg1); RestoreSimRate
  (48) and SetSimRate's undo call vt+0x80 (rate 0). While the rate is set, the controller's per-frame update
  (8256E4F0) posts OnRequestSetSimRate {timestep, count 1} every frame; the Simulation (82857EC8) turns the timestep
  into a wall-clock tick rate (integration stays 1/60) and returns to 1/60 two ticks after the posts stop. So arg1
  is a tick rate: "30" plays at half speed, "45" at three quarters. The handlers' stop (82579B90) restores it too.
- Ours: `presentation_run::SimControl` (timestep, `restore`), `SimRateHandler` for both types,
  `Presenter::sim_timestep` / `reset_sim` (tutorial cleanup); the challenge system posts
  `SimulationRateRequest { timestep, ticks: 1 }` each frame into `CameraRuntime::presentation_rate_requests`, applied
  with the camera's requests by the physics clock (`physics/clock.rs`), not online (as the camera's).
- NOT RETAIL YET: FixSkaterInAir, PauseSim / UnpauseSim, input restrictions and objective evaluation while paused
  are still logged stubs; whether our audio follows the slower ticks is not checked.
- Tests: `presentation_run` (a rate holds until RestoreSimRate or reset, an unreadable rate is ignored); the clock's
  own tests cover the 30 Hz period.
- Laptop presentation-sim-rate (2026-11-10 08:38): build OK, presentation 6, tutorial 2, clock 8 pass. All 22
  SetSimRate lists are hint rows (tut_sksc_03 objectives 8 / 10, tut_sksc_05 rows 5 / 6, tut_sksc_06 row 1) or
  failure rows (09, 11, 12): none fires in a verify run. tut_sksc_05 / 06 load 0 hint rows because their conditions
  call unported APIs (`Tricks.BeganTrick`; `ObjectiveAir.GetTimeToGrind`, `ObjectiveAir.IsDoingSpecificBoardAdjust`).
  Not seen in game yet.

<a id="tutorial-callout"></a>

### Tutorial callout (2026-11-10; b945 §2, b866 §1, b964 §5 step 6)

- Retail [code b945]: ShowTutorialCalloutHUD (type 20) writes the tutorialinstruction component's instruction
  (82D29B60): no key = a text-only tip (always marked changed), with a key only a new key or text changes it; the
  component becomes visible. HideTutorialCalloutHUD (21, 82578A80) writes key `none`, text `#` and makes it
  invisible. The component update (82667A70) sends hudtutorial's SetState: text only 12 on first show, 11 while shown,
  13 for `#` or after its shown timer reaches 10000; gesture mode 10, 11 for `none` / `tutorial_none`, hide for
  `clear` / `tutorial_clear`; 77 on a stance change.
- Ours, step 1: `skate_core::challenges::callout` (`Instruction` show / hide with a revision instead of the changed
  flag, `Panel::update` giving the states in order, plus the generic HUD show 246 / hide 236 when the visible byte
  changes; `CalloutParams.text_timeout` 10 s as a mod-replaceable default).
- Ours, step 2: ShowTutorialCalloutHUD / HideTutorialCalloutHUD handlers write the presenter's shared `CalloutHost`
  (instruction + panel; `Presenter::update` runs the panel and logs `PRESENTATION callout state <n>`); tutorial
  cleanup hides it (`Presenter::reset_callout`). `hud_challenge::take_callout` moves the states, the text and the
  resolved gestures into `ChallengeHudState.callout` (serializable; a running count so no state is sent twice);
  `Component::Tutorial` (hud2/hudtutorial) sends them as SetState in order and answers GetTutorialHalText (the text
  through the movie's language), GetTutorialGesturesButtons (`[count, gesture, button, connector, ...]`) and
  PlayerStance_IsRegular. Gestures: `TrickGuide::callout_gestures` (scoring_trick record slots, else scoring_tutorial,
  loaded by skate-data's trick guide reader; up to 4, until the first empty slot; button remap by stance), from the
  `TrickGuideData` resource the menu stage inserts.
- HUD imports (`hud_imports::HudLibraries`): hudtutorial imports "Gesture Item" / "Button Item"
  (source/controls/gesture_item, button_item2). The challenge HUD now loads every imported library from the menu
  movie set (with shared.json merged, as the menu stage does), links them into the movie, runs the libraries' root
  init actions first (class definitions, `Object.registerClass`), binds registered classes by linkage name, runs class
  constructors, and handles `createEmptyMovieClip` / `attachMovie` on its clips; imported shapes join the HUD's
  shape map with texture paths re-rooted at `../menu-movies`. prepare_hud.py writes HUD `exports` as bare ids, so the
  HUD side drops them from the import table.
- NOT RETAIL YET / open: the 10000 timer read as milliseconds [inferred]; the player stance is always regular
  (PlayerStance_IsRegular true); in the data-gated test the movie draws no units after 246 + 10 (the label and the
  gesture item are set and visible), a test-side gap: in game it draws (below).
- Laptop tutorial-callout (2026-11-10 08:17): build OK; callout, trick_guide, presentation, tutorial, hud_challenge
  and the data-gated test pass. tut_sksc_01: the hint3 tip logs 246 + 12 at the hint and 13 exactly 10.0 s later;
  the shots show the retail panel ("Coach Frank Says", the coach portrait, "3 pushes = Max speed") on the right
  and nothing after the timeout. tut_sksc_11 (verify run, 40 s): no callout state (its gesture tips are failure rows
  and nothing failed), so the gesture row is not seen in game yet; it is covered by the data-gated test only.
- Tests: `challenges::callout` (gesture tip once and repeat no-op, stance 77; text tip 12 / 11 / timeout 13 / `#`;
  hide, clear stays cleared, none); `presentation_run` (callout events reach the HUD state in order, cleanup hides);
  skate-data `trick_guide` (scoring_trick / scoring_tutorial class hashes); data-gated
  `the_tutorial_callout_shows_its_text_and_gestures` (desktop 2026-11-10: label "Do a kickflip", then "Turn", hidden
  after 13 / 236; with the libraries linked, mcGestureItem0 visible for the kickflip tip and hidden for a text tip).

### The DPO: killed it time and position (2026-11-08, b995 items 2a / 2b, b997 sections 1 and 2)

- Retail [data b982, code b997]: setup_heat sets DPO 1 when the heat has a KilledItTime (DPOSetTitle
  ID_CHALLENGE_RACE_KILLED_IT_TIME, DPOSetIcon KilledIt = 1, DPOSetFormatType Time = 2, DPOSetParams(0, KilledItTime);
  the dpo movie switches to its countdown mode on that title) and DPO 2 = ID_DPO_POSITION, Ints = 0, (1,
  NumCompetitors). run_heat's RacePassTimerUpdate (every 1.5 s, re-armed) puts the local place in DPO 2. The formatter
  `8266AF68`: numerator Ints / Points / Players a plain integer, Distance ID_COMMON_DISTANCE, Time the time text;
  denominator Ints ID_COMMON_NUMBER, Points ID_DPO_STR_POINTS, Players ID_DPO_STR_FINISHED, Distance
  ID_COMMON_DISTANCE, Time the time text. The movie shows "#" + numerator and the denominator as given; it never draws
  the icon (item 3).
- Ours: `skate_core::challenges::dpo` (DpoFormat with the retail values, `icon`, `numerator` / `denominator`,
  `time_text` as M:SS.hh); race_run emits `DpoKilledItTime`, `DpoPosition { of }` (Params.competitors = AI racers + 1),
  `RacePassTimerUpdate` (Params.pass_refresh_time 1.5) and `DpoHide` at cleanup_heat; `race_host` fills
  `DpoSlot { title, icon, value: DpoValue { format, numerator, denominator } }` and the place from
  `RaceTracker::rank`; `hud_challenge::dpo_texts` turns a value into the texts through the movie's language
  (`fe_language::format_string`). Own the spot still writes its texts directly (no value).
- NOT RETAIL YET / open: the time text's digit counts are filled at runtime and were not read (M:SS.hh is what the
  separators give, b997 medium confidence); without a KilledItTime the position DPO is the only one, in slot 1 of
  retail's two or slot 0 of one, is open (b995 2b); ours shows it as the single DPO.
- Tests: `dpo::tests` (position texts, time text), `race_run::tests::setup_heat_sets_the_two_dpos_and_racing_refreshes_the_position_every_one_and_a_half_seconds`,
  `hud_challenge::tests::dpo_values_become_the_movie_texts_through_the_language`.

## Change: race AI racers (`challenges/race_ai.rs`)

The AI side of a race (b978 port steps 5-6), in progress. Done so far:
- Setup exports every challenge family's AI lines (research b969): `data/content/aipaths/AIPATH_Challenge_<set>.rx2`
  in `miscload.big` are RW4 arenas holding AIPATHDATA, the same objects as the city tiles, so they go through the
  same reader into `private/living_world/challenge_paths/<set>.bin` + `index.json` (the pack and index layout of
  `skater_paths`; the index's `districts` are the sets, e.g. `ch_race_dwtn_02`). 59 sets on the disc: the 10 race sets
  (8 to 51 lines, flags 7 / 0x47, as b969 counted) and the follow the leader, contest, film, photo and 1-up sets.
  `tools/asset_pipeline/living_world_skaters.py` `challenge_line_sets`; the existing outputs do not change.
- `crates/skate-core/src/challenges/race_ai.rs`: the line-up (82566740 + setup_code, b987 / b992: the player is id 0,
  AI ids 1.. take the row's competitors after a forward Fisher-Yates, at most 6 racers), the per-racer state the graph
  drives (PinAI / UnpinAI, StartLapPaths leg, StartPath, finish), the leg part of the line test (82456970, b972: flag
  bit 9 + leg keeps a line off that leg; no retail line sets it) and the race rubber band (8246F0F0, b969:
  `d = ETA(player) - ETA(racer)` ramps 0 at 2 s to 1 at 5 s, negative when the racer is ahead, the player flag caps it
  at -1) with the speed shape (82470830: `s + 0.3 s shape + 0.125 s factor`). The tunables are data (`BandTuning`).
  Tests: `race_ai::tests`.
- In play (`crates/skate-game/src/race_ai_host.rs`, driven by `race_host.rs`): a race with competitors loads its set
  `ch_<race id>` and the racers' looks (ai_characters `look` -> characters_marquee) and shuffles the line-up from the
  session's population seed. The run's commands carry race_shared's AI calls: TeleportToStart places each racer at
  its start slot on the line starting within 4 m (`replay::choose_line_from`, b972 §4; the racers placed before it
  count as riders, so the branch score's crowding term spreads neighboring slots over the lines) and pins it (a
  racer already out is replaced, RepositionAI). Racers ride only their set: the set's ids are their line filter at
  branches and line ends (`NpcLineFilter`, the challenge path filter of b972 §3; without it a racer took no line at
  its first line end and left, laptop run 2026-11-07). Clock and filter are attached when the racer spawns. Racers
  skip the ambient leave fade at a dead end (8246EA90 is the ambient skater's leave; a racer takes its set's next
  line instead [inferred]). RaceReset adds them to the tracker (AddPlayerToRace), RaceStart lets them go
  (UnpinAI + StartPath), RaceStop pins them, the end removes them (UnspawnAI) and their lines. Racers are the living
  world's challenge AI skaters (`LivingWorld::spawn_scripted`, retail kind 3): spawned through the normal records,
  kept apart from the ambient roster and its culling, and while one rides the ambient skaters stop
  (`TickInputs::scripted_skaters`, 8245C4A8). Every world tick their positions feed the tracker (ETA, rank), a split
  volume containing a racer moves its progress, the finish calls FinishPlayer and the run's OpponentFinished (so
  finishing behind one loses, b984), and the band adds or removes cursor frames (`NpcRaceClock`; held frames while
  pinned).
- Racers as full skaters: a racer is spawned at its own start slot (setup_heat's RepositionAI; `NpcRaceClock::start`
  = slot position and heading) on the line whose start lies nearest it, and switches to the physics tier there at once,
  placed facing the slot heading. While pinned (PinAI) its placement hold does not count down, so it stands still with
  no path following or stuck watch; on release the hold's last tick gives the spawn push along its line's node 0 step.
  The push test's "left its place" anchor follows a pinned racer, so the settling on its slot (0.3 to 0.6 m, laptop
  run C) does not refuse the start push.
  The slot placement lands onto the ground like a travel (`SkaterRuntime::fit_next_arrival`, retail fit-up `82598428`,
  b475): race_dwtn_02's slots are authored 1.8 m above the ground, and the racers dropped in from there before the
  push (laptop F1 burst 2026-11-08). Two racers may share a line (race_dwtn_02: 3 line starts, 5 slots) but never a point. Racers ride over the ambient
  cap, so placed racers without a body top up the loaded-body pool on the loader thread during the countdown and wait
  for it ("race rider waiting for a loaded body"); before, the two bodies over the pool size loaded on the game thread
  at the release (laptop 2026-11-08: 206 + 196 ms, a 410 ms frame). A released racer is a full skater at any distance
  and is never handed back for distance (retail races its AI as full skaters). There the line-end fallback of the physics tier
  applies (`82458968` mode 1: no set line within 4 m -> the nearest valid start at any distance, b972 §4), and the
  band moves the push speed instead of the cursor clock: the race host sets `NpcRaceClock::scale` =
  `shaped_speed(1, 0, factor)` (k2 = 0.125, the shape term `[pc+4]+0x34` unread and 0), the push target is the line
  speed times it before the avoid clamp (`82470830`, b969). Splits and finishes test the racer's deck position
  (`NpcSim::deck_position`), not its line sample.
- Test runs: `SKATE_PLAYER_ON_FOOT=1` keeps the player standing off the board: every placement (`SkaterRuntime::travel_fit`:
  race slot, teleporters, challenges, menus, mods) lands off the board, and the map spawn (built on the board) gets one
  off-board press at tick 30 unless a placement already put the player off it (`controls::step_off_for_test`). Before
  it the unattended runs' player rolled away on the board (all 2026-11-08 laptop runs), which moves the branch scores'
  player distance. Mods: `sdk.player.teleport{on_board = false}` (sdk/ENGINE_API.md).
- Laptop race_dwtn_02 runs 2026-11-08 (unattended, player drifting on the board): base replay-tier racers 4/5 finished (64.85 to
  70.93 s); physics-tier racers with own slots, start push and loader-thread bodies 4/5 (66.35 to 73.67 s), worst
  frame during the race 19.7 ms. OPEN: line `..00` ends at node 26 (about [376, 67, -9.5], after the node 17 jump)
  with no line of any kind starting within 10 m (`NPC_SKATER_CHAIN_FAR`: the nearest are ambient lines 10.5 m away
  and 5 m higher), so a racer that reaches that end chains to an unreachable start, goes lost and bails; whether
  retail's racers take a junction branch before it is not checked yet.
- NOT RETAIL YET: before the physics tier, a racer that branched onto a side line with no set line starting within 4 m of its end stopped
  there (desktop run 2026-11-08: racer 3 at a race_dwtn_02 line end 41.6 m from the nearest start); a racer's tricks and
  capabilities come from its look's profile, not its race profile (b993 §1.2); racers use no gate ribbons, speech or
  leaderboard yet (item 6).

## Change: the end dialog (`challenges/end_dialog.rs`; research b920, b923)

Retail's offline end dialog is FE screen 4 (vtable 82302750), opened when a challenge graph calls
FrontEnd.OpenContinueRetryDialog (82854360): tutorials, lot activities and the objectives_shared families (films,
follow the leader, photos). Own the spot never opens it (b929: ownthespot_shared_mlui has no such call and no
hudcontinue / hudretry handler; its end_options only picks complete or restart, as `ots.rs` does). Rules ported to skate-core `EndDialog` (engine free):
- Four option slots, after a reset [10 Continue_Retry, 11 Retry_Quit, 12, 12] (825ECD18); graphs overwrite them
  with SetEndChallengeOption (82855548). 10 resolves to Continue on a pass and Retry on a failure, 11 to Retry / Quit
  (825ECF80). The count is the slots before the first 12 (825BC6A0).
- Movie (init 825F0CE8): a failure shows challengefailed_small (template 36); a pass with two or more result rows, or
  with the challenge's force flag (Bool 0x03B86F8338D5EC92, false in all shipped data), shows endchallengeoptions
  (33); otherwise endchallenge_small (34).
- Keys (825F12E8): A slot 0, B slot 1, X slot 2, Y slot 3, with no count check. Continue, Quit and MoreActivities
  close and post hudcontinue after the outro; Retry / Rematch post hudretry, after a pass behind the rematch confirm
  popup 217; ChangeDifficulty pushes FE 45 (825F13F8).
- Close (825F0F98) resets the slots unless the dialog was left for a settings screen.
- Lot activities (b923): pass with MoreActivities [10, 11, 18, 12], without [10, 11, 12, 12]; fail with both
  [10, 11, 18, 19], without MoreActivities [10, 11, 19, 12].
Option text ids (table 8221EB28) are `default_texts`, our data. Tests: `default_slots_resolve_by_result`,
`lot_activity_slots_and_reset`, `movie_choice`.

In play (tutorials): the run's end options emit `OpenEndDialog` (opened as passed, b920) and wait; `challenges.rs`
opens `EndDialogScreen`, the menu stage (`retail_menu_stage.rs`, `END_DIALOGS`) opens endchallenge_small or
challengefailed_small, shows it after its intro and maps Apt keys 2 / 3 / 5 / 4 to slots 0..3 (825F12E8). The pick
goes back to the run as HudContinue / HudRetry. `end_dialog_host.rs` answers Mission.GetNumEndMissionOptions /
GetEndMissionOptionText ("#" + text id) / GetWinState / Results_GetKilledIt / GetTitle and
HUDComponents.Results_GetNumPlayers (0: our families write no result rows, b925). Logged `SKATE_CHALLENGE end dialog`
and `RETAIL_MENUS end dialog slot <n>`. Test runs: `SKATE_VERIFY_TUTORIAL_PASS=1` (with `SKATE_VERIFY_TUTORIAL`) completes the objectives once they evaluate, so the dialog and the career's reward chain run unplayed; `SKATE3_RETAIL_MENUS_KEYS` presses through them. Laptop runs 2026-11-07: the dialog draws (endchallenge_small; its movie path must match the exported key's case, the
stage's lookup is case sensitive), its buttons read the localized option texts (text natives answer `#` + the
localized text), its title comment comes from the movie's own `Math.random()` pick, and A picks Continue and completes
the tutorial. NOT RETAIL YET: the stage side is still behind `SKATE_RETAIL_END_SCREENS=1`; without it, or without the
movies, the run continues at once; Retry after a
pass skips the rematch confirm popup 217; retry's fade and music restart (b923 §3) and ChangeDifficulty (FE 45) are
not ported. Own the spot has no dialog in retail either (b929).

## Change: the sign-up poster (`challenges/poster.rs`)

Research b932, b935, b939. Retail shows a poster before a single challenge starts: FE screen 0x1B (the lot list's
screen) on page 0, template 23 SignUp_Poster, movie `challenge/challenge_signup`, movie state 20 (a lot row gets
state 10, the lot list). A on it starts the challenge (825EE188 -> 82D52B60); B closes it and cancels the pending
sign-up (82D52C98). Retry and the pause restart never show it, and an activity picked on the lot list (a chained
start) skips it (b939). Rules in skate-core `Poster` (engine free):
- The CIC menu (825C1E88): Start first, then Pick Teammates when career teammates are usable, then Sign Up Co-op
  when the challenge is co-op capable; `#NO OPTION` alone when the team gate (825EE7A8) fails.
  SetCICMenuOptionIndex stores 2 for entry 1 without teammates (index 2 always means co-op).
- Keys (825ED410): A on Start (gate passed) starts, B cancels; the first accepted press locks the poster.
Tests: `options_follow_the_team_and_coop_flags`, `entry_one_without_teammates_is_coop`,
`a_starts_b_cancels_and_the_first_press_locks`, and the host's `natives_answer_from_the_view`.

In play: one challenge in reach plus D-pad Up opens `PosterScreen` (`challenges.rs` `sign_up`) instead of starting at
once; the menu stage opens the movie, shows it after its load / intro call (or the same 30-update fallback as the end
dialog), posts state 20, sends A / B to `PosterScreen::press` and every other key to the movie. `poster_host.rs`
answers SignUpPoster.GetSignUpTitle / GetSignUpDescription / CanSignUpToChallenge / GetCICMenuOptions /
SetCICMenuOptionIndex / GetCICMenuOptionIndex / IsCompetitorChallenge / GetCompetitors, the poster's Mission natives
(owned, killed, killable, rewards, teammates, competitors) and HUDComponents.SignUp_GetNumObjectives /
SignUp_GetObjectiveTitle. Logged `SKATE_CHALLENGE poster <id>`, `RETAIL_MENUS show poster`, `RETAIL_MENUS poster key`.
Data (research b947): the export adds `description_arg` (Int8 0x0AF3F024F18EAB9D, the description's one "%d"
argument) and `objectives_enabled` (Bool 0x858D73CD6085C8EC) next to `description_id`; killable (827CDA70: type 0x13
or a non-empty ObjectivesKilledIt) and the owned / killed board sales (OwnedItReward / KilledItReward +4) come from
the career's `progression.json` (`killable`, `challenge_rewards`); own the spot objective titles are
`ID_OBJECTIVE_BEAT_SCORE` with the owned / killed point requirement (8264D130, type 9). Arguments are printed as
plain decimals before substitution [the substitution itself inferred]. NOT RETAIL YET: behind
`SKATE_RETAIL_END_SCREENS=1` like the end dialog; the co-op bonus is online only (0 here); the killed objective row
is shown when the killed points are above 0 [inferred stand-in for a non-empty ObjectivesKilledIt]; no teammates or
co-op offline, so Start is the only option. Several challenges in reach keep our pick
popup; the map's Sign Up is not ported yet (retail's arrival path skips the poster, b913).

## Change: player autopilot (`autopilot.rs`; ours, not retail, 2026-11-08)

- Why: a testing aid for challenges and the career (the brain attempts a challenge and the result is logged), and a mod
  surface (scripted sequences). Retail has no player autopilot; the brain it runs IS retail's NPC skater AI (doc 26e).
- How: the brain does not press pad buttons; it gives a body the AI record (`SkaterRuntime::ai_physics`), its trick
  signals (`PlayerControls` intents) and `ai_driven`, all on the player's own types. The autopilot spawns a scripted NPC
  skater (`Possessed`) and `possess_in` / `possess_out` (around `npc_sim::simulate`) swap the player's physics context,
  runtime, controls and camera into its `NpcSim` and back (`npc_sim::swap_player`, `GamePhysics::swap_player_context`),
  so the brain and its step run unchanged on the player; scoring, challenges, career and camera see a normal player.
  The player's own controls and physics step stand down while it drives; one arming tick avoids a double step.
- Start (ours): the nearest node of any line within 30 m, a node heading away from the player counting 10 m farther,
  never the last two nodes; the NPC is pinned at the player's spot like a racer at its slot (it takes a body at once, at
  any distance) and released once it holds the player. Retail starts AI skaters at a line start.
- Guards: the possessed NPC's body never goes to the pool while it holds the player's (`pool_from`); prop ownership
  stays with the main physics during the swap, so the autopilot pushes no props (NPC steps never do); the possessed NPC
  is hidden and silent (the player is drawn and heard); it does not avoid its own body, and other NPCs avoid the player,
  not its hidden line sample (`npc_avoid`).
- Speed (ours): the record's velocity step (flag bit 28, `82C05868`, up to 0.2 m/s per tick toward the recorded
  skater's velocity) is dropped for the autopilot (`ai_record::without_speed_pull`); position and facing still steer.
  With it the player gained speed uphill with no push (user, desktop Industrial runs: 2.4 m/s over 12 ticks on a 12 %
  rise, no push); without it every ground speed-up in a 3 min Industrial run has push ticks or a downhill, at the same
  pace (843 m at 6.0 m/s vs 908 m at 6.1 m/s). `NPC_SKATER_SPEED` now counts the push ticks in its window.
  The laptop recheck (autopilot-speed-20261108-2133) left three more launches, now off for the autopilot too: the spawn
  push after the handover and after every bail respawn (0 to 4.6 to 6.9 m/s in a few ticks; it now pushes off,
  `NPC_SKATER_PUSH ... none: the autopilot pushes off`) and the recorded jump arc (`with_trajectory`: the recorded
  skater's launch speed, 4.5 -> 10.4 m/s going up); its jumps use its own ollie physics. Desktop DownTown 3 min after:
  926 m at 6.2 m/s over 14 lines, 4 bails.
- Jumps (ours, `skate-core` `living_world::autopilot_brain`, rules in `JumpRules`): the laptop recheck of the change
  above (autopilot-launch-20261108-2158) showed 0 pushless ground rises, but more bails, mostly from the air (DownTown 4
  -> 6, 1 -> 3 from the air; Industrial 2 -> 5), with takeoffs 2 to 4 m/s under the recorded jump speed: retail stops
  pushing 40 frames before a trick and its NPCs get the rest from the speed pull. Now, within 240 recorded frames of a
  jump, the autopilot pushes toward that jump's recorded flat takeoff speed and keeps pushing until 12 frames before it
  (`jump_ahead`, `push_target`, `trick_cuts_push`); the recorded arc is used again only when its own flat speed is at
  or above the recorded takeoff speed, so the arc never adds speed (`keeps_arc`). 45 frames before a jump and under
  85 % of its takeoff speed, it switches to another line whose node is 1 to 12 m ahead, at most 4 m to the side (the
  brain's lost radius is 6 m), within 2.5 m of height, heading its way within 45 degrees and whose own next jump its
  speed can make (`should_switch`, `switch_line`), on the next tick through the brain's own cursor restart; with no
  such line it tries the jump. A push adds about 2 m/s per push at walking speed but little above 8 m/s, so jumps
  recorded at 10 to 12 m/s (built from hills) are out of reach on the flat. Logs: `AUTOPILOT_JUMP` at every takeoff
  (flat speed, the needed speed, arc used, approach ticks below it, ticks pushing, why the others did not push),
  `AUTOPILOT_SWITCH` per switch decision. Desktop DownTown 3 min (the runs repeat tick for tick): before 3 bails at
  6.16 m/s, with the push to takeoff speed and the arc rule 6.38 m/s, with the switch at 45 frames 6.46 m/s (3 bails,
  1 from the air, 4 switch decisions, 2 switched). Deciding at 90 frames with a 20 m radius switched 11 times, left the
  path (LOST) twice as often and fell to 5.05 m/s; with the 4 m side limit at 90 frames 4.90 m/s: kept at 45.
  Laptop (autopilot-jumps-20261109-0029, against 9a93bbe1): bails 13 -> 8 and air bails 6 -> 3 over the three maps at
  the same speed; but 3 of 17 switches were lost at once (the new node 8 to 11 m away), one switch came at the takeoff,
  one mid-grind onto an 11 m/s uphill line, and a stopped autopilot climbed 3 m up a 20 % slope with no push. Now a
  switch needs the node within 5 m in all (under the 6 m lost radius), 15 to 45 frames before the jump, riding
  (PhysicsGround), and a recorded speed at most 1.5 x its own (at least 3 m/s); and the record's position step keeps
  only its flat part across the line (`ai_record::sideways_position_step`): it steers onto the line but never carries
  the board along or up it. Desktop 3 min after: Industrial 5.20 m/s, 3 bails, none from the air, no switch lost;
  DownTown 7.14 m/s, 0 bails. One flat pushless rise left (Industrial, 3.3 -> 5.5 m/s in 7 ticks, on the path).
- Races (ours, 9ecd94c7): on every race start (`TeleportToStart`, counted in `Races::placements`) the brain lets go
  of the player, waits 10 ticks for the slot teleport to land and engages again at slot 0, picking only the race's own
  lines (`PopulationState::scripted_lines`, the AI racers' set); it is held at its slot (`NpcRaceClock` not moving)
  until `RaceStart` (`Races::started`) and races with no rubber band (it is not one of the AI racers). Splits and the
  finish fire from the player's own body as always. At the end it logs `AUTOPILOT_RESULT race=<id>
  completion=<..> time=<s> place=<n> brain=<look>` and goes back to an ambient line (the race's lines leave with it).
  Dev: `SKATE_CHALLENGES=1 SKATE_RACE=<id> SKATE_AUTOPILOT=1` on the race's map. Not run yet in a race (the desktop
  install had no challenges export; refreshed 2026-11-09).
- Teleports (ours): the player's deck moving more than 10 m between the brain's step and the next tick (any teleport
  outside the brain: `--teleport`, the teleport menu) restarts the brain where the player arrived; the brain's own
  bail respawns do not count. Before, the laptop's Mega-Park run (autopilot-megapark-20261109-0733) engaged 11 ms
  before `--teleport` landed, found itself 315 m from its line and walked for 108 s until a bail respawn put it back.
- Race fixes (2026-11-09, laptop autopilot-races-20261109-0812 and desktop runs): a line switch only picks the
  race's own lines (`switch_line` gets `PopulationState::scripted_lines`); a simulated skater's branch and line-end
  choice now honors its challenge line filter (`NpcLineFilter`; before, the simulated path used only the per-character
  lines and chained the autopilot off the course); the autopilot's line clock is held while it waits at its slot (as
  the AI racers' `NpcRaceClock::held`), so at GO its line has not run 30 m ahead. `AUTOPILOT_RESULT` gains
  `outcome=won|lost|time up` (the record's completion is 0 for a lost race too; retail has no lost value). Desktop
  after: race_dwtn_01 2nd in 109.47 s (Busenitz 90.07 s), race_univ_01 3rd in 89.73 s (Haslam 71.32 s, Drehobl
  77.17 s), both on the race lines to the finish.
- Learning from bails (ours): two bails on one line within 60 s and the autopilot leaves that line for the nearest
  other line (`AUTOPILOT bailed <n> times on line ... leaving it`); not in a race; a teleport or race start clears it.
  It came from Danny Way's map, where it respawned onto the mega ramp lines at a standstill and bailed 9 times in 2 min.
- Pumping: the autopilot pumps down slopes (doc 26c "DLC map lines and opt-in pumping").
- Wedges (ours): moved less than 3 m in 6 s while riding (not bailed, not held at a start) and it gives the spot up
  (`AUTOPILOT wedged: ...`): outside a race the line joins the avoided lines, and it picks the nearest other line; in a
  race it picks the nearest node of the course again. For the curb lip and planter ledge wedges of the laptop runs.
- Direction (2026-11-09): the autopilot is meant to play like a capable but imperfect player that still makes
  mistakes (it is a test aid); it is not tuned to dodge cars.
- Line lists in free roam (ours, a test aid): `SKATE_AUTOPILOT_LINES=<id>,<id>,...` starts on the first line; when
  the brain leaves the current line (line end, branch, re-pick) the player is teleported to the next line's node 0 and
  the brain engages there; the list loops (`AUTOPILOT_LINES left <id>: line <k> of <n> <id> at its start [...]`). Not
  in a challenge with its own lines. Desktop DownTown 160 s with three lines: two full loops; line `...68a2` wedges at
  nodes 30-35 every pass (a repeatable spot for the wedge rule).
- Dev: `SKATE_AUTOPILOT=1` engages at the start, `SKATE_AUTOPILOT_CHARACTER=<look>` picks the brain's character
  (default danny_way), `SKATE_AUTOPILOT_LINE=<32 hex digits>` the start line; any pad input releases the player.
  `AUTOPILOT` log lines: engaged / released and every 2 s position, speed, state, line and node.
- Verified (desktop, DownTown, 60 s): it walks to the line, then rides six lines in a row with airs, no bails; before the
  avoid fix it slowed for its own body (31 avoid entries naming the player), after it none. A stop at a turn (speed
  5.4 -> 0.5 m/s while it left and rejoined the path) is the retail brain's own.
- Laptop (autopilot-20261108-2053, the pull still on): DownTown 722 m over 15 lines, Industrial 262 m over 8, University
  336 m over 7 (the given line's fallback works); no panics, the player visible and followed by the camera; wedged on a
  curb lip (~28 s) and against a planter ledge (~14 s, then a bail), bails from the air, one bail from a car.
- Next (`.claude/todo/player-autopilot.md`): wedge / crawl recovery, traffic, races as a racer, free-roam line lists,
  the challenge-aware layer, `sdk.player.autopilot` (the `JumpRules` there too).
- Tests: `autopilot::tests::the_pick_is_the_nearest_node_heading_the_players_way`,
  `autopilot::tests::a_line_id_parses_from_32_hex_digits`, skate-core `ai_record` (the record without the speed step),
  skate-core `autopilot_brain` (push target, trick cut, arc rule, line switch), `ai_record` (the sideways position step).

## Files

- `tools/asset_pipeline/vlt.py` (`field_bytes`), `tools/asset_pipeline/test_vlt.py`
- `tools/asset_pipeline/challenges.py` (new), `tools/asset_pipeline/test_challenges.py` (new),
  `tools/asset_pipeline/objective_lua.py` (new), `tools/asset_pipeline/test_objective_lua.py` (new)
- `tools/asset_pipeline/install.py` (the export step, after teleporters)
- `tools/asset_pipeline/map_volumes.py` (`strict`), `tools/asset_pipeline/retained.py`, `test_retained.py`
- `tools/prepare_hud.py` (the six challenge movies)
- `crates/skate-core/src/challenges/` (mod, timers, ots, ots/tests, objectives, predicate), `crates/skate-core/src/lib.rs`
- `crates/skate-game/src/challenges.rs` (new), `scoring_runtime.rs` (banked sequences), `main.rs`, `app.rs`
- `crates/skate-game/src/challenge_records.rs` (new), `crates/skate-game/src/hud_challenge.rs` (new), `scoring_hud.rs` (hosting), `hud_central.rs` (colors shared)
- `crates/skate-game/src/startup_loading.rs`, `app.rs`, `splash.rs` (`SKATE_BACKGROUND_WINDOW`)
- `crates/skate-game/src/modding/challenges.rs` (new), `modding/mod.rs`, `crates/skate-mods/src/vm.rs`, `api.lua`,
  `tests/engine_api.rs`, `sdk/ENGINE_API.md`; `crates/skate-game/src/living_world/opt_ins.rs` (the opt-in)
- `crates/skate-game/src/popup_host.rs` (`signup_state`), `retail_menu_stage.rs` (overlay state per prompt,
  `MenuStageSet`), `teleporters.rs` (state 2, prompt order), `apt_stage.rs` (overlay test for both states)

## Verification

- `python -m unittest tools.asset_pipeline.test_challenges tools.asset_pipeline.test_map_volumes
  tools.asset_pipeline.test_retained`: OK.
- Export against the disc: `ots_dwtn_01` start (-132.75, 30.98, 143.53), owned 1000, killed it 1500, boundary
  `ots_dwtn_01_challengeboundary_01`, spot `ots_dwtn_01_scoreboundary_01`.

## Moddability

Challenge ids are the retail key strings. Mods read, start, abort, hide and re-tune challenges through
`sdk.challenges` (see "the mod entry"); adding a whole new challenge (own volumes, start, points) is still open.

## Open questions

- Who opens the owned it / killed it screens (FE templates 26 / 27): not found statically; ours shows the banner
  movies through the OwnedIt / KilledIt components. A recomp trace on OpenScreen during an own-the-spot win is planned.
- The timer text below a minute (the format's digit bytes are 0 in the static image).
- Box versus stored bounds for the trigger test (`ots_indu_02`).
- The sign-up gate: the owner (+4) that decides show or hide, what each veto's condition means (b856 open list).
- The multi pick: retail's sphere slot order, the element list's capacity, the input mode pushed while the popup is up
  (826DB498 with 3), and whether the Sk8Popup movie draws the title (b851 open items).
- What the retail flagged list (+0x34) means; what the Challenge Map and the sign-up marker read from the record.

## Contests

2026-11-11, world/challenges; research `.local/research/b1009-contest-machine.md` (single player, the retail graph `contest_shared` re-read) and `b877` (rows, bindings).

**Problem.** The street / vert contests and demos (eChallengeTypes 8 Street, 0xF Vert, 2 Demo; the career's contest family) had no runner, so their challenges, their results and the contest music could not run.

**Retail [data, b1009].** Rounds: setup, countdown (3 s), the round (RoundTime), end of round, results, cleanup, back to setup. The round score is the best single score, or with CumulativeScoring the sum of scores of at least CumulativeMinScore; AI scores in normal rounds are multiplied by AIScoreAdjustment. SoloRun: the player's best of SoloRunNum runs counts, each AI rolls one score in SoloRunAIScoreMin..Max at setup. TimedRunSingle: the round timer running out scores the player 0. End of round: rank by round score, place points N - i into the totals (a tie with the place above keeps its points). Result: a demo wins unless a round fell under its DemoTargetScore; otherwise the player wins with the top total, ties win. Completion Owned or Abandoned (no killed it). Music triggers: 0 Countdown, 1 Start, 6 AlmostOver (RoundTime - 3), 7 Over (end of round), 8 / 9 Win / Lose.

**Change (step 1).** `skate_core::challenges::contest_run` (`ContestRun`, `Params`, `Round`, `RunType`, events and commands in the race run's style; graph constants as overridable defaults; the SoloRun RNG seeded by the authority).

**Change (step 2, the export).** `tools/asset_pipeline/challenges.py` `contest`: challenges.json `contests`, 26 rows from the disc (street_ind_01 has no rounds). Field names matched by hash against the round rows (class 1E04F6C0A209B06A, reached through the params row's ContestRounds): RoundTime (Int16 s), RunType (0 normal, 1 SoloRun, 3 TimedRunSingle, 4 TimedRunMulti, 5 unnamed), ContestVariant, AIScoreAdjustment, CumulativeScoring / CumulativeMinScore, SoloRunNum, SoloRunAIScoreMin / Max (Int16: real ranges such as 3500 / 2000, not the 0 of b877's float reading; min can exceed max), DemoTargetScore (5000 on demo_indu_01), StartLocation / WaitLocation, RoundBoundary, SpotVolumes, Paths; competitors from the bank row as for races. The rows b877 could not convert (street_indu_02, vert_indu_02 / 03) export. Each round's start locator has its world matrix (`start_matrix`) from the contest's hull (some rows name another contest's locators: that hull), all 26 resolve. `music_playlist`: the challenge's aud_playlists key (attrib 874EC914B4D34708; no disc contest sets it, so retail's default 614B4D4D82510720).

**Change (step 3, the host).** `crates/skate-game/src/contest_host.rs`: `SKATE_CONTEST=<id>` arms the contest on its own map (dropped on a map change) and feeds the run the player's banked sequences (`Challenges::banked_this_frame`, taken once per frame by the challenges system). `ContestRun` emits `TeleportToStart` at setup_round (SoloRun: before each run); the host puts the player at the round's `start_matrix`. Music: the contest's triggers go to `music_zones` `contest_trigger` (the playlist branch of 8248B258 -> 8249E1E8): 0 Countdown stops the music, 1 Start queues the challenge's playlist on the soundtrack graph, 12 StopChallengeMusic gives the music back to the zones, which wait while a contest owns it. Tests: contest_host rows to params, music_zones contest trigger.

**Not retail yet (labeled in the code).** The AI competitors' scores are a data-driven stand-in from the round's AIScoreAdjustment / SoloRun ranges (retail's AI riders land their own tricks); the round results and end options screens are not ported, the host continues at once; every banked sequence counts (no trick-type, boundary or spot volume gates); trigger 6 AlmostOver's 3000 ms fade to level 2 is not played (no stream fades yet).

**Verification (step 3, 2026-11-11).** Desktop dev build, `SKATE_CHALLENGES=1 SKATE_CONTEST=street_dwtn_04` on DownTown (install given the step 3 contests export, restored and hash-checked after, settings too): three 90 s rounds, the player placed at each round's start locator, a 3 s countdown per round, place points 3 / 6 / 9 to all four with an idle player (the stand-in scales the player's 0), end Owned (ties for first win, b1009 §1). With the soundtrack on no contest music plays (retail: Interactive Mode only); in Interactive Mode trigger 1 queues the challenge's playlist and its first song plays. Trigger 0 at an env start is dropped while the saved music mode is still being applied at map load (test start only).

**Change: the HUD timer text and its font pair (ask 600, 2026-11-11).** The time text follows retail's duration formatter 0x8290EB38 with the descriptor 0x82FDE25E, whose digit widths are written at runtime: a recomp watch run read `02 02 02 02 00 ':' ':' '.'` (hours, minutes, seconds, fraction widths, sign, separators), so the text is `01:12`, `33`, `05`, nothing at 0 (hours and minutes only when non-zero or after a printed field, a field after a printed one padded to 2, the HUD call drops the fraction). Ported as `timers::DurationFormat` (data, so a mod can change it); before it showed `1:12` and `5`. The timer movie places only the APT font "Futura Shadow" (the shadow atlas, glyphs in alpha only); retail 825D6B68 draws it black and the paired native font futuraheavy over it in the text color, looked up in the whole font table even when the movie places no such font. `tools/prepare_hud.py` exported only the fonts a movie places, so hudtimer and hudtutorial drew the shadow atlas alone in the text color: a pale blur. The HUD export now adds the pair's atlas (as the menu export already did); data-gated test `the_timer_is_opaque_after_its_intro` (opaque after the intro, a black futurashadow pass and a futuraheavy pass in the text color); desktop frame shows a crisp `01:06`. Takes effect at the next Refresh Game Files.

**Change (step 4, the contest HUD).** `contest_host` drives the race HUD movies: the 3-2-1-GO from the countdown state's 3 s HUD countdown (`ContestRun::countdown_hud_ticks`), the round timer from RoundStart to RoundStop, and the leaderboard set up at setup_round (one row per competitor: the player's name, then each competitor's look name id from `skater_profiles` as for races) and refreshed on every score change with the round score and round place (descending, a tie keeps the place above). The layout follows setup_round's UseIntScore (contest_shared `setup_round.stateenter`@742-807, read 2026-11-11): Sequence and its trick-type variants, Air_Spin, Best_Line, Air_Only and Grind_Only use RankNameScore (3, whole points); Grind_Length, Speed, Air_Distance, Air_Grab_Time, Air_Height and Air_Time use RankNameFloatscore (6). eContestType values by the image's string order (b877, inferred; the same order gives eRunType's values the export uses). The export's `variant` is read per round. The leaderboard row clip is the row's place in rank order (tied ranks by row id), so tied rows each get their own clip (a race never ties). NOT RETAIL YET: the float score text (two decimals). Verified on the desktop 2026-11-11 (street_dwtn_04, Sequence rounds): 3-2-1-GO, the round timer, and the leaderboard with the player highlighted and the three pros, all rank 1 at 0.

**Change (step 5, the round results).** results_screen without elimination or teams (contest_shared `results_screen.stateenter`@220-450 and 776-850, read 2026-11-11) in the end dialog (FE 4, `OpenContinueRetryDialog(true)`): two result blocks, the round results shown first (menu RoundResults 1, title ID_CHALLENGE_COMMON_ROUND_RESULTS, rows by round score, layout BestTrickNonElimInt 5 or Float 4 by UseIntScore, option Next; HighlightReel only with highlights, none recorded yet) and the contest standings (menu FinalResults 0, title ID_CHALLENGE_COMMON_CONTEST_STANDINGS, rows by total place points, BestTrickNonElimInt, options Continue / Previous). Place = position in score order; int layouts write the score as the int, float layouts -1 and the score as the float (Results_SetPlayerData). Column titles from the round row (`ResultsScreenHALIDCol1..3`, exported as `result_columns`: ID_COMMON_RANK / NAME / SCORE). Ghidra: SetEndChallengeOption 82855548 writes the slot into the active block (R+0x3C), so each block has its own options; Next 825F1768 activates block 0, Previous 825F17C0 block 1, both refill the movie (SetState 6). Ours: `results::contest_results`, `end_dialog::Action::Page`, `EndDialogScreen::other` + `refill` (the stage sends SetState 6). Continue goes on to the next round; without the dialog movies the run continues at once; the autopilot continues after 3 s. Verified on the desktop (street_dwtn_04, injected keys A / B / A / A): round results with Next, standings with Continue / Previous, both pages refill, Continue starts round 2.

**Change (step 6, the end options).** end_options without elimination or teams (contest_shared `end_options.stateenter`@0-80, 192-212, 399-440, 659-757, read 2026-11-11), single player: Result Win -> Owned, else Abandoned; one block (FinalResults), slots Continue_Retry / Retry_Quit / ChangeDifficulty after a loss, title ID_CHALLENGE_COMMON_CONTEST_STANDINGS, rows by total place points (BestTrickNonElimInt), the last round's column titles, no rows for a demo; `OpenContinueRetryDialog(won)`, so a loss shows the failed dialog. Continue completes the contest, Retry starts it again (setup_challenge). NOT RETAIL YET: a win with an OutroNIS plays the NIS first in retail (NIS not ported). The end options open in the same frame the last round's Continue closes the round results (same movie, so the stage neither closes nor reopens it): every contest results open asks for a refill (SetState 6). end_round turns the HUD off: RoundStop hides the timer and the leaderboard. Verified on the desktop (full three-round run, background window): every round's results page and continue, the end options reload (a fresh Results_GetNumPlayers after the end), Continue completes Owned.

**Change (step 7, AI contest riders: riding).** Retail scores the AI competitors like the player: `run_round.sequencecomplete` maps every skater's own `cMsgSequenceComplete` to its competitor (`GetCompetitorIndex(PlayerID)`) and scores every competitor (contest_shared @0-439), and the native scoring update posts that message for every skater in its player list (82DF18B0 -> 82DF0878, from each skater's own score state). So the competitors must ride and land their own lines. `crates/skate-game/src/contest_ai_host.rs`: the contest's competitors (its competitor rows, in order, as competitors 1..) ride the contest's challenge lines (`ch_<contest id>`, the race racers' line export), spawned and pinned at setup_round, released at run_round (AI.UnpinAI + StartPath), pinned at end_round, removed at the end or on a map change. Each round filters the set by its `Paths` (below). setup_round then calls `AI.RepositionAI(id, CurRound.StartLocation, id, false, 0)` for every competitor (contest_shared setup_round.stateenter @430-442), so competitor n stands on child n of the round's start locator, like the race heats (b992); the export writes each round's `start_slots` (6 per round on the disc). Each rider spawns on its slot and takes the line the path follower picks from there (the ones placed before it count as crowding). Which rider stands on which slot is rolled when the contest starts (the race line-up's Fisher-Yates over slots 1..N, the player on 0), from a seed that is new every start (`race_ai_host::start_seed`, also used by races now; `SKATE_CHALLENGE_SEED=<n>` pins it for repeatable runs). The first desktop run had all three riders on the round start (one shared point, so one line): stacked bodies failed their first physics tick ("Grind graph received inconsistent completed filtered outputs") 226 times, each retry rebuilding the full skater (about 150 ms, 150 hitches over 100 ms in the round); on their own slots the same round had 0 physics errors and 3 hitches over 100 ms (the promotions themselves). Contests without an exported line set (e.g. street_dwtn_04) run without riders. Their scores (step 8: a scoring runtime per rider, the AI landing bail roll 82460460) replace the stand-in next.

### Contest line gates (step 9)

**Retail** (contest_shared, read from the disc graph). `run_round.sequencecomplete` @17-206 scores `M.SequenceScore` unless the first failing gate zeroes it (an if / else chain): out of the round boundary (`inBoundary`), a round with spot volumes and a line that hit none (`hitSpotVol`), or the round's trick type missing: Sequence_Grab / Grind / Footplant / Handplant / Hippyjump need `Trick.GetNumTrickType(PlayerID, eTrickType) > 0`, Sequence_Flip needs `didFlip`. The state behind them: setup_round @173-224 starts every competitor in bounds, in no spot volume; the trigger handlers set `inBoundary` on entering / leaving `RoundBoundary` and count `inNumSpotVol` up / down on the round's `SpotVolumes`, an entry also setting `hitSpotVol`; `sequencebegin` sets `hitSpotVol = inNumSpotVol > 0` (not in Best_Line) and clears `didFlip`; `aircompleted` sets `didFlip = GetCompletedAirNumFlips != 0`. For the local player a wrong-type line adds to a counter that warns at 3 (`ShowModifiedCentralMessage("ID_CONTEST_WRONG_TYPE", n)`, n = 2 air rounds, 3 grab, 4 grind, 5 spin, else 1; `ChallengeSpeech` 307) and resets; a scoring line resets it. `Contest.eContestType` values come from its enum builder 8284C8A8 (each name pushed with its number: Sequence 0, Sequence_Grab 1 .. Grind_Only 16), which proves the order b877 inferred.

**Change.** `contest_run::{Gates, gate_line, WrongTricks, wrong_type_modifier, variant}` (pure, tests); the export writes each round's `boundary_volume` and `spot_volume_shapes` (the shapes from the hull the round's locators came from); banked sequences carry their completed tricks by catalog category and a body flip flag (`scoring_runtime::BankedSequence`; NPC skaters' through `NpcSkaterEvent::SequenceComplete`, plus `SequenceBegin`). `contest_host` tests every competitor's position against the volumes each tick of the round and applies the enter / exit edges, opens the sequence on the scoring runtime's rising edge, and gates every line before `UpdateScore`. [inferred] The tested point is the hips (the player's hips joint, a rider's root + 1 m); retail tests the skater's own trigger body. [inferred] eTrickType names matched to catalog categories by name (Grab 4, Grind 5, Footplant 13, Handplant 6, Hippyjump 10). NOT RETAIL YET: what the warning's modifier does to the text, speech line 307, the air variants' own handlers (aircompleted scoring).

**Verification.** Unit tests `line_gates_follow_the_contest_graph`, `three_wrong_lines_warn_and_a_scoring_line_resets`; export check: every contest round resolves its boundary and 1 to 8 spot volume shapes.

### AI rider scores (step 8)

**Retail.** Every skater's score state posts its own `cMsgSequenceComplete` (82DF18B0 -> 82DF0878 for each skater in the player list), and contest_shared `run_round.sequencecomplete` maps `msg.PlayerID` to the competitor and calls `UpdateScore(CIndex, SequenceScore)` (x AIScoreAdjustment for non-players, b1009 §0).

**Change.** The NPC sim drains each simulated skater's banked sequences (`scoring_runtime::take_banked`, the same output the player's contest scores come from) into `NpcSkaterEvent::SequenceComplete { id, score, wiped_out }` (also a mod event `npc_sequence_complete`); `contest_host` scores a rider's non-bailed sequences for its competitor id, and the run applies the adjustment. The stand-in (`AiScoreStandIn`) now only scores competitors without a riding rider (a contest without an exported line set, or a rider that was not spawned).

**Verification.** Desktop run street_dwtn_01 (hidden window, muted, install restored and hash-checked): riders banked their own lines (e.g. 440 -> 337.5 with the round's 0.75 adjustment), 63 landing rolls with 4 bails, 0 physics errors.

### AI landing roll

**Retail.** `AIPathCtrl_AirLatch` 8246A700 (at 0x8246A9B4), on the landing edge: when the posted trick (path controller +0x34C, our `TrickSlot::trick`) is a flip, fingerflip or grab (categories 2 / 3 / 4) other than the coffin grab (0x3C), and the skater's AI is not ambient (`[[skater+0x724]+0x9C] != 0`), `AITrick_RollLandingBail` 82460460 computes `chance = profile[+16 + 4 * tier] * profile[+40] ^ half_turns` (tier = the trick's `Sk8::Score::ETrickDifficulty`, scoring_trick +0x74, 0..3, else the +28 slot; half turns = score state +0x3018) and posts WipeOutRequest when `chance < rand01 * 100`; the posted trick is cleared. The profile block holds `ai_skater_profiles` `Hash_913D2B5F07880189` (+16), `Hash_F83A843573435E6C` (+20 and +28), `Hash_06025FDC568AA983` (+24), clamped 0..100, and the optional `Hash_1DD7BEA0203DD57B` (+40, clamped 0..1; a missing one reads the zero default block 0x830D0850, checked in Ghidra). On the disc these are on all 193 records, e.g. `busenitz_ftl` 100 / 100 / 100 with 0.0 per half turn (lands every straight trick, bails every spin), `ali_boulala_skate` 85 / 75 / 70 with 0.8.

**Change.** `skate-core` `npc_tricks::LandingSkill` (formula, clamps, fallback tier) and `rolls_landing`; `skate-data` reads the trick difficulty (`scoring::Definition::difficulty`) and each profile's landing skill (`SkaterTrickProfiles::landing_for`); `npc_sim` captures the half turns every airborne tick and, at the landing edge of a physics-tier challenge rider (one with an `NpcRaceClock`; ambient skaters never roll), rolls with the NPC's own seed and the world tick and posts WipeOutRequest on a bail (log `NPC_LANDING_ROLL`). Mods change the odds through the profile data.

**Verification.** Unit tests `landing_roll_matches_the_retail_formula`, `only_non_ambient_flips_grabs_and_fingerflips_roll`; data-gated `every_ai_profile_carries_its_landing_skill`.

### AI path filters

**Problem.** The first contest rider run (laptop, street_dwtn_01, 2026-11-11) logged "no line near the round start" for every rider in rounds 1 and 2, so no contest rider was spawned: the riders searched all 84 lines of `ch_street_dwtn_01` for a start within the 4 m chain radius of the round start, with no fallback.

**Root cause.** Two retail steps were missing. (1) `AI.FilterPaths` (8284A4C8) turns each name of a row's `tRoundPaths` (contest round `Paths`, race heat `AIPaths`) into a 16-byte filter id (824570F0) and the rider may only take lines whose id matches one of them (82457620); the flag at +0x320 == 1 (FilterDisabled) turns the filter off. (2) The path follower's line pick from the skater's body point runs in mode 1 (82458968): a filtered start within the radius by branch score, else the nearest start (b972 §4); we had the fallback off.

**Evidence.** Ghidra + recomp code of 824570F0 and its token helpers 82457928 (longest name in a table), 82457780 (location), 82457850 (subset), 824579C8 (number), 82457A20 (token up to `_`); name tables 0x830432FC (`am`, `ch`, `ob`, `te`, `*`, `am_*` = id byte 0) and 0x830432B0 (17 kinds `1up` .. `vert` = id byte 5). A name is `<prefix>_<kind>_<location>_<instance>[_<subset>]_<index>`: the subset is only taken when a further token follows, so every disc name selects exactly one line (kind, instance, subset, line index; the location bytes are stored but not compared). Data check (`.local/tools/path-filter-check.py` on the desktop install): race_indu_01's 27 names select its 27 lines, race_univ_01's 12 its 12, every other race heat has the filter off.

**Change.** `crates/skate-core/src/challenges/path_filter.rs`: `filter_id`, `matches`, `RoundPaths::valid_lines` (tables as data). The setup export writes contest rounds' `ai_paths` as `{names, all_paths, param}` like race heats (`tools/asset_pipeline/challenges.py`; the old bare name list is still read, as filter on). `contest_ai_host.rs` and `race_ai_host.rs` pass the filtered lines as `valid_lines` (also as the riders' `scripted_lines`, so they keep to them when they chain) and search in mode 1 with the fallback. A rider that still finds no line logs "not spawned"; each spawn logs its line id and position.

**Fix: filtered-out lines are not a fallback (2026-11-11).** Desktop runs showed the riders and the test autopilot chaining onto ambient lines at a line end. Ghidra of `82458968` shows why that is wrong: each line first goes through `82458860` (the path filter, then the 0x200 object overlap), and a rejected line is skipped before anything else, so it is never a candidate nor a fallback; the fallback's valid / invalid split is the mask / leg test `82456970`. Ours passed the filter as `valid_lines`, which made filtered-out lines fallback candidates. Now `replay::FilteredLines` hides them from the line search (lookups by id still see every line) in both tiers (`npc_skaters` step, `npc_sim` line end chain), and `valid_lines` is the per-character mask again. Test `a_filtered_out_line_is_never_a_fallback`.

**Fix: the player stands on slot 0 (2026-11-11).** The contest teleported the player to the round's start locator; street_dwtn_01 round 2's locator lies 1.3 m under the plaza (the map's collision there is at y 16.8 to 17.6, `skate-data` example `collision_at`), the arrival fit casts from 0.75 m above the asked point, found nothing and the player fell through. setup_round places every competitor with `RepositionAI(id, StartLocation, id)`, the player (competitor 0) on slot 0: the player now goes to `start_slots[0]` (the locator when a round has none). The test autopilot is told about the teleport (`Autopilot::host_teleported`) and picks a line where the player lands; its own deck jump test missed it while the brain drives.

**Moddability / multiplayer.** A mod's contest round or race heat names its riders' lines with the same strings (or turns the filter off); the filter is a pure function of the row data and the line ids, so every client computes the same set.

**Verification.** Unit tests `path_filter` (names parse like retail, the match rules, a round picking its named lines); contest tests pass. In game: laptop milestone 3 (needs Refresh Game Files for the new export).

**Next.** the trick-type and boundary gates on the player's sequences (b1009 §2); the AlmostOver fade.

**Verification.** Unit tests `contest_run` (place points and ties, best vs cumulative and the AI adjustment, ties for the top total win with the music order, SoloRun best run and AI rolls, TimedRunSingle zero and the failed demo).

### AI riders at a line end (2026-11-12)

**Problem.** In a contest the riders all took the same line and jammed at its trick spot, and at a line end they ran back to a start on foot to ride a line again (asks 614, 621).

**Retail.** A rider's line end uses the same search as any AI skater (b1017): `8246D3C0` -> `8246C7F8` -> `82458968` mode 1 with the round's path filter; with no filtered start within the radius it takes the nearest one at any distance, and the lost test (4.5 m) then hands it to the NavMesh controller, which walks it there. Riders on the same line keep apart only through the avoider (slow down / speed up behind a skater, `82465578`, `82470830`). The contest script adds no line-end handling of its own (b877, b1009: only the out-of-bounds timer's `RepositionAI` to the start location).

**Change (the user's contest rule, not retail's chain).** `PopulationState::contest_riders` lists the running contest's riders (set by `ContestAi::place`, cleared by `remove`). At a line end a contest rider searches only `contest_lines` (`npc_sim.rs`): the round's lines it has not ridden this round and no other rider is on; with none left, the ones it has not ridden; with none of those, any but its own. The usual mode 1 search then takes the nearest start among them, and the rider rides there on the board: the lost walk-back and the mode 7 step-off are off for contest riders (the stuck tracker's resnap / respawn stay). Riders that end up on one line keep apart with the avoider, as in retail. Log line `NPC_CONTEST_LINE` (lines done, lines to choose from).

**Riding over.** From a chain until its body is within the line end's lost radius (4.5 m) of the new line's start node, a contest rider's follower stays on that node: no search, no line end, the line waits (`ChainScratch::riding_over`). Without it the windowed search (`82455BB0`, 4 nodes a tick) crept along the new line towards the body, and street_dwtn_01's round lines end where the riders stood, so they chained again within a second (28 of 121 chains). A rider not there after `RIDE_OVER_TICKS` (12 s) is placed on the line by the stuck respawn.

**Files.** `crates/skate-game/src/living_world/mod.rs`, `living_world/npc_sim.rs`, `contest_ai_host.rs`.

**Verification.** Desktop contest runs street_dwtn_01 (2026-11-12): line changes within 3 s of the last 0 of 61 (was 28 of 121); 45 of 61 rode to their next line, 12 were placed after 12 s; 0 physics errors.

**Arrival further along (2026-11-12, ours).** The placed riders had ridden onto their next line part-way along (on the path, inside the lane, at segment 4 or 5) and never came within the lost radius of its start node, so they were placed after 12 s although they were riding it (run 13:22: 15 placed of 61). A rider the follower has on the path at or past the chain node now counts as arrived. Run 13:36: 48 of 61 reached their next line, 10 placed (was 15), 0 chains within 3 s of the last, 0 physics errors. The 10 left had chained to a line whose start was 25 to 40 m away (the nearest allowed start, as retail, b1017) and did not get there in 12 s.

### Player pinned through the countdown (2026-11-12)

**Retail.** setup_round pins every competitor (PinAI), run_round unpins them and end_round pins them again until the cleanup (b877). For the local player, `AI.PinAI` / `AI.UnpinAI` (8284ADC8 / 8284AF20) post `cMsgPinPlayer` (0x7949DAC1, flag 1 / 0) instead of setting the AI skater's bit; the PlayerUI handler (82899C90) forwards {player id, flag} as record type 7, which sets or clears the same skater bit (+0x770 0x04000000, research b116 §3). While it is set the skater's controller skips its tick input (b121, b78), and the actor's physics setup (82593640) copies the bit every tick into the physics packet's force-brake byte (+0x2A2C, our `force_braking_10796`), which brakes a skater riding on 3+ wheels once it is nearly stopped: a skater placed on a sloped start slot stays there. Readers of the bit (recomp scan, `.local/tools/find-field-bit-readers.ps1 -Offset 1904 -Shift 6`): the AI path controllers, 82593640, 8259FC28 and the actor state hash 82593BA8. Races pin the player in setup_code and again at the finish, and unpin at GO (b982).

**Change.** `Contests::player_pinned` (setup_round through the countdown, and from end_round on) and a race before GO / after the finish feed one `input::player_pinned`. While it is true the published pad actions are neutral (a human player cannot push off early), the player's physics profile requests the force brake (`GamePhysics::set_pinned` -> `AnimationProfile::force_braking`, which existed but was never set), and the autopilot holds (`Autopilot::hold`). Mods still override the actions on top of the pin (`modding::override_actions`), so a mod can drive or free the player.

**Files.** `crates/skate-game/src/input.rs`, `physics.rs`, `autopilot.rs`, `contest_host.rs`.

**Verification.** Desktop run: run 1 started 18:40:06.977, the autopilot pushed off at 18:40:06.993 having moved 0.000 m during the hold. Human pin (dev aid `SKATE_TEST_PRESS=a` pulses push and logs the skater's movement while pinned; `.local/tools/desktop-contest-run.ps1 -Press a`), street_dwtn_01 before the force brake: rounds 1 / 2 moved 0.081 / 0.064 m while pinned and 9.2 / 8.3 m in the 3 s after GO; round 3 (a downhill start slot) rolled 10.3 m during the countdown. With the force brake wired (diagnostic once a second while pinned): rounds 1 / 2 settle below 0.1 m/s on 4 wheels and the brake bit is on; round 3 is still 0.98 m/s on 2 wheels 16 ticks after the arrival, reaches 7.3 m/s, the brake never engages and it rolls 10.3 m. Round 3's slot 0 is the retail locator child 0 (`82910EB0` copies child[slot] as is, b989), facing down a slope of about 20 degrees (1.55 m drop over 4.1 m); the teleport's ground fit (`82598428`) keeps the locator's rotation and only drops the position. The brake rule (state ticks > 5, speed < 0.1, Ground state, 3+ wheels) cannot catch a skater there, so as ported retail would roll too. OPEN: confirm in retail / the recomp whether the player rolls at that start.

### Autopilot push and brake (2026-11-12, ours)

**Problem.** The test autopilot pushed and then braked straight away (ask 612).

**Cause.** Its push aimed at the recorded speed here (or a jump's takeoff speed) while its brake fires above 1.25x the slowest recorded speed in the next 60 frames: it pushed 6.4 -> 8.5 m/s at 7.0 recorded and braked 0.6 s later for 5.4 ahead.

**Change.** `autopilot_brain::push_speed`: the push aims no faster than that slowest speed ahead unless a jump needs more; the brake's margin absorbs a push's overshoot. Test `push_aims_no_faster_than_the_slower_stretch_ahead_unless_a_jump_needs_it`.

**Verification.** Brakes within 2 s of a push: 6 of 38 (was 11 of 21).

**Repeated brakes (2026-11-12, ours).** That run had 52 brakes (all the autopilot's; NPC skaters keep retail's `sub_8246B810` rule), 18 of them a repeat at the same spot under 3 s after the last: a brake released at 1.25x the speed ahead, the downhill sped the skater up again and it braked again (node 39: 2.4 to 2.6 m/s for 1.7 ahead, every 1.5 s). `BrakeRules::release_factor` (1.0): a running brake goes on down to the speed ahead itself.

**New lines in a contest (2026-11-12, ours, ask 613).** The autopilot rode the same few lines back and forth at one spot (round 3: lines 14 and 15 five times in a row). In a contest it now picks its next line like a contest rider (`contest_lines`: lines it has not ridden this round first; the ridden-line lists live while the contest runs). Run 13:46 (`.local/tools/autopilot-line-revisits.ps1`): round 3 12 distinct lines, 0 revisits (was 5 / 8); round 2 7 / 0; round 1 3 revisits from mid-line switches for a jump; 0 physics errors. Test `a_running_brake_holds_down_to_the_speed_ahead`. Desktop autopilot run street_dwtn_01 (5 min, 3 rounds): 40 brakes, 9 repeats (was 52 / 18); 0 physics errors.

**Open.** Mod reach: the rule is on for every contest; a per-contest switch (Lua API 2) is not there yet.

### Films and follow the leader: the shared machine (2026-11-12, groundwork)

**Retail.** Films (ChallengeType 7, 26 rows), anywhere films (0x27, 10) and follow the leader (0x17, 7) run the state graph `objectives_shared`; photos (0x26, 0x25) run `photo_skate2` = it plus a photo state and shot picker (research b875). Follow the leader's own branches: the distance meter at the countdown, the leader reaching `FTLEndVolume` opens a 10 s window (the leader is pinned), the window running out ends the run Abandoned, a wipeout does nothing (`AllowPlayerBail`), the objective's distance failure (`Leader.GetDistanceToNearestAISkater() >= 100` on ftl_univ_02) ends it (b1006).

**Change.** `crates/skate-core/src/challenges/film_run.rs`: the graph as an engine-independent machine in the own the spot / tutorial runner style (deferred state changes, enter on flush): 18 states (initial .. complete), the per-type rule table (`Rules::retail`), `ChallengeResultSelector` (films repair and go on unless every objective is complete or the film timer ran out; follow the leader ends unless a boundary it has was left [inferred, b875 §5]), the timers (countdown delay 1.5 s, PrefilmTime, the film timer with its low-time warning, out of bounds 5 s, owned pending 0.5 s, wipeout 1.5 s, objective failed 1.5 s, AI restart 1 s / 0.2 s, the FTL window 10 s, check_ai poll 0.25 s), the result mapping (Win -> Owned, Kill -> Killed, Lose -> Abandoned). Every row value and graph constant is in `Params` with the family defaults (`Params::retail`), so a mod row can change them.

**Not yet.** Follow the leader is hosted (next section); films and anywhere films are not. SimulPlay and the photo states are left out.

**Verification.** Tests `follow_the_leader_wins_when_the_owned_objective_completes_and_waits_half_a_second_for_a_kill`, `follow_the_leader_ends_abandoned_when_the_ten_second_window_runs_out_and_a_bail_changes_nothing`, `follow_the_leader_too_far_behind_fails_the_objective_and_ends_the_run`, `a_film_runs_out_of_time_and_a_wipeout_rearms_instead_of_ending`, `leaving_a_films_boundary_counts_down_and_coming_back_cancels_it`, `a_kill_beats_a_pending_owned_and_the_retail_values_are_data`.

### Follow the leader in play (2026-11-12)

**Retail.** Follow the leader runs the shared machine above with Kind Follow (b1006): the player starts on slot 0 of the start locator and the leader (competitor 1) on slot 1. Both are held through the countdown, then the leader rides its recorded line. The owned objective completes when the player enters the end volume. The objective's inline failure (`FailureObjectives`, checked after the with-events rows) ends the run when the leader gets too far ahead (`Leader.GetDistanceToNearestAISkater() >= FTLMaxFollowDistance`: 40 / 50 / 100 m by row). When the leader's body reaches the end volume, it is pinned and a 10 s window opens. While it rides, the follower band (b969) changes its speed from 0 at 5 m to plus or minus 12.5 % at 15 m from the player.

**Change.**
- `tools/asset_pipeline/challenges.py`: `follow()` exports `follow_the_leaders` (6 rows: start with slots, end volume, max follow distance, row params, competitors, owned / killed objective rows). Objective rows now also carry `failures_inline` (condition + failure text).
- `crates/skate-core/src/challenges/api.rs`: Leader API (`GetDistanceToNearestAISkater`, `GetDistanceToSkater(n)`, `Add/RemoveFollowIndicator`) on `SkaterFacts::competitor_distances`.
- `crates/skate-core/src/challenges/race_ai.rs`: `FollowerBand` + `follower_band` (data, a mod can change or switch it off).
- `crates/skate-game/src/challenge_objectives.rs`: inline failures load after the with-events rows of their objective (row index past them, `ObjectiveData::inline_failure`).
- `crates/skate-game/src/ftl_host.rs`: the host. It handles start via `SKATE_FTL=<id>`, the leader on the contest rider code (`contest_ai_host.rs`, now with a per-rider speed scale), distances, the leader's end-volume entry by its body, the player pin, the 10 s window on the timer HUD, the failure central message, the end dialog and the career result.

**Killed it group (2026-11-13).** No retail follow the leader row has a killed it list: the export (`ObjectivesKilledIt`, inherited through the family) is empty on all 6 rows, none of the 126 killable challenges in `progression.json` is a follow the leader, and every FTL killed reward is 0. The graph still builds a killed group from the list like the films (make_objectives, b875), so `ftl_host.rs` now runs a row's killed list as its own group next to the owned one: its completion ends the run with Kill (the shared machine cancels a pending owned), its failures and hint rows work as the owned ones, and the end dialog shows killed. A mod row can add one. Test `a_killed_it_list_runs_as_its_own_group_and_completes_on_its_own`.

**Sign-up (2026-11-13).** The sign-up prompt (b849 / b851) is the same for every challenge kind: the SignUpManager lists every challenge whose sign-up sphere (Location, GlobalActivateRadius) holds the player. It was fed own the spot rows only. Each follow the leader row's export already had its locator and radius (3 m on all 6 rows), so the prompt now takes `Offer`s (id, title, type, sphere) from own the spot rows and follow the leader rows alike, with the same order, overlay state (1 for a non own the spot), pick popup, mod / career gate and poster (without own the spot's objective titles, which 8264D130 adds for type 9 only). A `ChallengeSignUp` for a follow the leader row starts it in `ftl_host.rs` when no challenge or tutorial runs, so the career's first challenge (ftl_univ_02 on University, b976) starts too. `SKATE_FTL` stays as a test aid.

Races and contests (demos too) join the same prompt. `challenges.py` exported their radius but not their Location; a shared `sign_up_location` now exports it for races, contests and follow the leader (the district's locators, else the challenge hull's). Fresh export: 10 / 10 races, 24 / 26 contests (street_dtwn_01 borrows another contest's locators, vert_skp5_01 has no map) and 6 / 6 follow the leader rows have one; contests and follow the leader rows are otherwise identical to the previous export. `challenges::SignUpFields` (flattened into the race, contest and follow the leader rows) builds each row's `Offer`; `race_host.rs` and `contest_host.rs` start a signed-up row when no challenge, tutorial or run of their own is going, like `ftl_host.rs`. The prompt is hidden while any challenge, tutorial, race, contest or follow the leader runs. Installs need Refresh Game Files for the race and contest locations (follow the leader had them already). Tests `a_race_or_contest_row_offers_its_sphere_on_its_own_map_only`, `rows_with_a_sign_up_sphere_on_this_map_are_offered_to_the_prompt`. Desktop run 2026-11-13 09:14 (University, `SKATE_VERIFY_CHALLENGE=ftl_univ_02`, no `SKATE_FTL`): the skater placed at the locator, the prompt listed ftl_univ_02, the press signed up, `ftl_host` started the run 4 ms later, the countdown released the player 1.5 s after, the follow indicator went on and the leader rode off with the player behind (verify shot).

**Not yet (NOT RETAIL YET, logged).** The band's sign (b969 open, ours: the player behind the leader slows it); discovery by the follow the leader boundary.

**Verification.** Tests `leader_calls_read_competitor_distances_and_push_the_indicator`, `inline_failures_fire_after_the_with_events_rows`, `the_follower_band_ramps_from_five_to_fifteen_meters_and_slows_a_leader_the_player_trails`, `rows_become_follow_params_with_the_end_volume_and_their_competitors`, plus the film_run tests above.

### Films in play (2026-11-13)

**Retail.** Spot films (ChallengeType 7, 22 rows on the three maps) run the same `objectives_shared` graph as follow the leader, with no competitors (b999). Offline there is **no film timer**: the graph adds the TotalFilmTime timer only in simul play (countdown.timerexpired@19-27), so a single player film has no time limit; the low-time speech still plays at TotalFilmTime minus FilmTimerLowLimit (175 s). A wipeout fails every objective, and 1.5 s later the objectives start over and the player goes back to the start; leaving the boundary for OutOfBoundsTime (5 s) also sends the player back to the start and the film goes on. The run ends Owned when every objective of the owned group completes (0.5 s owned pending), Killed when the killed group completes. Anywhere films (type 39, 10 rows) run the same graph but start where the player is (b1003).

**Change.**
- `film_run.rs`: the film timer runs only with `Params::simul_play` (default false, a mod can switch the timed variant on); the low-time speech runs whenever the row sets FilmTimerLowLimit (anywhere films set none: no warning). The out-of-bounds handler reads the selector before it sets the out-of-bounds flag (timerexpired@0-3), so a film repositions once; the flag left set repositions again at the selector's next call, as in retail.
- `challenges.py`: `follow()` exports films and anywhere films too (`films` in challenges.json: the start, boundary, owned / killed objective rows, sign-up spot and the row values, FilmTimerLowLimit as Int16; no start or hull needed for anywhere films; AllowPlayerBail defaults to true only for follow the leader). 32 rows: 22 spot films complete; left out for no hull: film_dwtn_cplaza_01, film_ind_01, film_skpk_univ_01, film_univ_09.
- `ftl_host.rs` hosts every objectives_shared row: the kind from the row's family, the film values over the family defaults, no leader and no AI wait for films, sign-up through the same prompt, `SKATE_FILM=<id>` as a test start. `SKATE_FTL` and `SKATE_FILM` switch the challenges opt-in on for that run, like `SKATE_CONTEST` / `SKATE_RACE` (they did not: a laptop film run with the opt-in off never started, 2026-11-13). `ResetObjectives` while running (a film's wipeout, objective failure or session marker) starts the objectives over at once; at a restart the countdown activates them. Leaving the boundary shows the retail "Return to Challenge Area!" text. A second reposition while the player is already on the way to the start is the same move.
- The objectives (2026-11-13 afternoon). The objective list on the HUD: each row's title is its tObjectiveDefinition words 3 and 4 (b947), shown only when DisplayInHUD, with its state (active, complete, failed) from the owned group. `challenges.py` exports the volumes the objective rows test (`objective_volumes`, shape from the hull) and the host arms them at the start; before, the map had none of them, so no objective could see the player in its spot. The films' flow mode 0 activates every objective at the start (b934), not only the first. Both groups use the tutorials' feed (`ObjectiveFeed`: skater facts, the frame's messages, one evaluation per 60 Hz tick).
- The test autopilot keeps to the recorded lines through the objective volumes, first the ones that do a trick inside one (mine, not retail; a test aid).

**Verification.** Tests `an_offline_film_has_no_time_limit_but_keeps_the_low_time_speech`, `a_film_in_simul_play_runs_out_of_time_and_a_wipeout_rearms_instead_of_ending`, `leaving_a_films_boundary_counts_down_and_coming_back_cancels_it` (one reposition), `a_film_row_runs_on_the_same_host_without_a_leader_or_a_film_clock`, `a_films_objectives_all_start_active_not_only_the_first`, `the_objective_hud_lists_the_shown_objectives_with_their_titles_and_states`, and `film_univ_02_grind_objectives_complete_from_the_exported_rows` (data-gated, SKATE_TEST_ASSETS: the exported rows and volumes; a clean line that grinds in the planter volume completes objective 2 and its HUD row, the 1st stairs' rail objective 0, a grind elsewhere or a bail nothing). Desktop runs film_univ_02 on University with the test autopilot (2026-11-13 11:00 and 11:09, 3 min each): both objective groups loaded from data, the player placed at the start, no film clock, the run live for 3 minutes; the autopilot rode out of the boundary 6 times and each time "Return to Challenge Area!" showed and 5 s later the player was back at the start. Desktop runs 14:30 and 14:52 (5 min each): the HUD lists "Grind down the 1st Stairs", "Trick Down the 2nd Stairs", "Grind the Planter", all active; the autopilot rides through the gap and grind volumes but never lands a grind or trick in them, so no objective completes in those runs. Not play-tested yet.

**Not yet (NOT RETAIL YET, logged).** Anywhere films: their objective rows resolve only in `data/db/skatercollections`, which the export does not read (b961 §1), so their rows are left out of play. The out-of-bounds central text id (ModifiedCentralMessage(OutOfBounds)) is not decoded. The filmer camera and the highlight reel of the film. Film objectives that call unported API (b961 §2: spin degrees, trick groups, unique tricks, ResetAllObjectives, manual distance and others) keep those rows from starting until the calls are ported.

## The distance meter

**Problem.** Follow the leader showed no distance to the leader: the meter movie was not exported and the host only logged it.

**Root cause / evidence (Ghidra, TU3; research b1006 item 8, b882 section 4).**
- The meter is FE template 45 `DistanceMeter` = `hud2/huddistance` (template table 0x8302F1A8). The movie is a copy of the timer movie (states 1 show, 2 hide, 3 `UpdateTimerHUD`) and reads `HUDComponents.DistanceMeter_GetData` (8265C978): [text +0x3C, flag +0x5C] of the HUD element `distance` (vtable 0x8230622C, built in 82659A10, flag starts at 1).
- `SetupDistanceHUDData(name, max, current)` (82850320) writes the group component's current (+0xC) and max (+0x10). The element's tick (826648D0) refreshes every 5th HUD tick (826647E8): flag 0 (red) when max >= 0 and current / max > 0.6 (0x821EE79C), else 1; text = `ID_COMMON_DISTANCE_PRECISION_ONE` ("{0:distance.1}") with the current value, then SetState 3.
- The `distance` text type (824FE848, registered by 824FDFC0): one decimal and `ID_COMMON_METRIC_DISTANCE` ("m"), or under Game Settings > Units imperial the value times 3.0 and `ID_COMMON_METRIC_NEW_FEET` ("ft"), the same formatter as the career stats.

**Change.**
- `tools/prepare_hud.py`: exports `hud2/huddistance` -> `private/hud/runtime/huddistance.json` (needs Refresh Game Files; without it the component is skipped and logged).
- `crates/skate-core/src/challenges/distance_meter.rs` (new): `flag`, `DistanceMeter` (setup, a 5-tick refresh at 60 Hz HUD ticks, any frame rate) and `MeterTuning` (retail values as data).
- `crates/skate-core/src/career_stats.rs`: `distance_text` shared by the stats and the meter.
- `crates/skate-game/src/hud_challenge.rs`: `Component::DistanceMeter`, `ChallengeHudState::distance` (serializable: shown meters, units, flag) and the `DistanceMeter_GetData` binding.
- `crates/skate-game/src/ftl_host.rs`: at the countdown's ShowHud(DistanceMeter) sets the meter up with the row's `max_follow_distance` and the distance to leader 1, then feeds it every frame; hidden at the end like the other components.
- `crates/skate-game/src/retail_menus.rs`: `imperial()` (also used by the Stats panel).

**OURS.** A row without a max gets -1 (never red); the HUD tick rate is taken as the FE cadence (60 Hz, doc 31). Mods cannot change `MeterTuning` yet (no API entry).

**Verification.** `red_above_sixty_percent_of_the_max`, `refreshes_every_fifth_tick_at_any_frame_rate` (skate-core), `the_distance_meter_shows_the_distance_and_turns_red` (the retail movie, ignored without the converted HUD). Not play-tested.

## The follow trail behind the leader

**Problem.** `Leader.AddFollowIndicator(1)` (the objective's first evaluation) was only logged: no trail showed the player which skater to follow.

**Root cause / evidence (Ghidra, TU3; research b1006, b966, b566).**
- `AddFollowIndicator(i)` (82C6CC00) calls `VisualIndicatorMgr_Add(mgr, i, 0)` (82D4F470): it removes any entry for `i`, then pushes `{id i, kind 0, active 1}` to the manager's actor list at +0x1548. `RemoveFollowIndicator(i)` (82D4F500) takes it out again. [code]
- That is the list the companion trail reads: `82D4FAD0` copies the active entries of +0x1548, `82D5D710` collects up to 4 over the groups, and the render side (`827AEC80`) draws each one as the ribbon trail behind that skater's board. Kind 1 (a called companion) uses style `trail_follow`; kind 0, the follow indicator, uses `trail_skatewith` (key 0x112D826AA8AD3C57). [code + data, b566 §2]

**Change.**
- `crates/skate-core/src/challenges/api.rs`: `FlowCommand::FollowIndicator { competitor, on }` carries the competitor number.
- `crates/skate-game/src/challenge_objectives.rs`: the calls are kept in order (`follow_indicator: Vec<(u32, bool)>`).
- `crates/skate-game/src/ftl_host.rs`: the followed competitors (an add removes the old entry first, as retail), published each frame as their living-world serials; cleared when the run ends.
- `crates/skate-game/src/visual_indicators.rs`: `FollowIndicators` (a resource a mod or another host can fill), drawn with the companion trail in the `trail_skatewith` style, companions first, 4 trails at most.

**Verification.** `leader_calls_read_competitor_distances_and_push_the_indicator` (the competitor number), `trail_style_found_by_its_hash` (both styles), plus the existing trail tests. Desktop run ftl_univ_02 (2026-11-12 22:41): `trail for skater #1 (trail_skatewith)` logged at the objective's first evaluation, the blue ribbon behind the leader in the screenshots.

## The leader's speech

**Problem.** The four hint rows of a follow the leader objective (`ftl_univ_02_0_0` .. `_0_3`) were evaluated but their presentation was dropped: no "keep up" / "you're right on me" lines from the leader and no start tip (b1006 §3).

**Retail / evidence (Ghidra, TU3).**
- The rows: `_0_0` every `Delay` = 60 evaluations asks `SpeechNoWait("Reda", 309)` when the leader is 15 m or more ahead (`FallBehindDistance`), `_0_1` id 308 between 3 and 15 m, `_0_2` id 310 within 3 m (`PassingDistance`), `_0_3` once at the start the tutorial callout `ID_CHALLENGE_TUT_SCHOOL_HINT_01` for 10 s. They share the objective's vars (`_0_0` counts all three timers) [data].
- `Speech_Play` (824A4EE8): ids of 5000 and up are NIS lines (nis.big); lower ids are cast / announcer / camera-man lines. For a cast character it goes to `guess_Speech_RequestCategoryLine` (824AC050): the id's main-cast event from the 73-row table at `8224F1B8` ([event, id]: 308 -> 7, 309 -> 8, 310 -> 9), asked of the active skater with that character (its bank and its near / far), else with the character's attrib words and flag 2 [code].
- `maincastspeech.big` holds the lines as events 7 / 8 / 9 (`308_follow_chat`, `309_follow_behind`, `310_follow_pass`) in each leader's voice (voice 30 = Reda, the same index as the speaker table) [data].

**Change.**
- `crates/skate-game/src/ftl_host.rs`: a hint row starts its presentation group on the shared presenter (as the tutorials, 82C755B8), and the objectives evaluate once per 60 Hz challenge tick instead of once per frame (at 260 fps the 60-evaluation `Delay` passed every 0.25 s). The tick rate is [inferred]: retail's evaluation rate is still open (b1006 §7).
- `crates/skate-game/src/presentation_run.rs`: `Speech` / `SpeechNoWait` with an id under 5000 map it through `CAST_SPEECH_EVENTS` (the `8224F1B8` table, retail values as the default) and queue a `CastSpeech` instead of looking for a NIS file; the event does not wait on it.
- `crates/skate-game/src/game_audio/world_speech.rs`: a presentation's cast line is said by the skater in the world with that voice (the leader), through the main cast like its other lines (its distance sets near / far).

**No skater of that voice in the world** (824AC050's other path): the request carries the character's own cast words and flag 2 (far) and plays on a PlayerSpeech instance with no world position (`cast_speaker`). OURS: the block's word order is the pros' (`cast_block`); 824AC050's own order (161E7AD7 / 14FD437D / AF7F / 6F2933E9 / D6EA428C) is not cross-checked.

**Tutorials too** (`crates/skate-game/src/tutorial_run.rs`): their objectives also evaluate once per 60 Hz tick now. A frame's edge facts (sequence completed, on / off board, air completed) and trick lists wait for the next evaluation and are spent on its first tick, so a frame without a tick loses none and a frame with two counts none twice. Verify run 2026-11-13 07:24 (tut_sksc_01): intro, hint row 3, the run as before.

**Verification.** `cast_speech_ids_map_to_main_cast_events`, `a_cast_speech_event_goes_to_the_main_cast`. Desktop run 2026-11-13 06:31 (ftl_univ_02, test autopilot, muted): the start tip shown, the hint rows firing about once a second, `309_30_follow_behind` playing on the main cast from the leader. Not listened to yet.

## The autopilot follows the leader (ours, a test aid)

**Problem.** In a follow the leader run the player autopilot rode its own chain of the challenge's lines: at a line end it took a far line start and walked there on foot, away from the leader (desktop run 22:41, ask 640). Once it took the leader's lines it rode up beside the leader and passed it, then fell back (ask 641).

**Change.** Not retail: retail has no autopilot.
- `crates/skate-game/src/ftl_host.rs`: the first leader's lines are recorded in the order it takes them (`leader_path`); `Autopilot::follow` is the line the leader took after the one the brain is on (`follow_line`), so after a bail the brain keeps to the path instead of walking to where the leader is now (run 23:06 walked 45 m to the leader's current line). A speed cap goes to the possessed brain (`follow_cap`): none 8 m or more behind the leader (`FOLLOW_GAP`), the leader's smoothed speed easing down to 75 % of it with no gap, 60 % once ahead, never under 4 m/s (`FOLLOW_MIN_SPEED`: a leader still at its start does not stop it).
- `crates/skate-game/src/autopilot.rs`: with `follow` set the brain starts on that line and gets it as its line filter (`NpcLineFilter`), so a line end takes only that line.
- `crates/skate-game/src/living_world/npc_sim.rs`: `NpcSim::speed_cap` caps the autopilot's push target and its brake target, never below the next jump's takeoff speed.

**Verification.** `the_autopilot_keeps_a_gap_and_never_passes_the_leader`, `the_autopilot_takes_the_leaders_lines_in_order`. Desktop runs: 22:51 (follow, no cap) rode the leader's lines on its board and owned the challenge after 88 s; 23:17 (path order, cap) took 0b, 02, 03, 07, 08, 09, 0a in order on its board, one 4 m step back onto line 03, one bail at 75 s; the 2 min limit ended it before the finish.
## Hall of Meat: the machine (2026-11-13)

**Problem.** Hall of Meat challenges (ChallengeType 0x1F, 25 career rows) had no run: no state machine, so a row could not start, score its best bail or end.

**Retail (research b1005, re-read with Lua 5.1 polarity; b876, b967, b998).** The career rows run the `hom_challenge` graph (parent objectives_multiplayer < objectives), not `hall_of_meat` (the online battles graph). Setup pins the player, routes the HUD to the challenge's Hall of Meat group (`HUD2.SetHOMHudGroup(Name)`, back to "main" at cleanup), shows the minimap and hides the trick display; the start is the reset marker, then a 1.5 s delay and the 3 s countdown. While running: every bail of an unfinished single player run ends with a teleport to the start (wipeout@46-77, wipeoutover@131-180) unless a session marker was used during the bail (usemarker@15-34); the best bail is kept (`Trick.GetHOMScore`, strictly greater) and becomes the highlight while unfinished. The owned group completing starts a 0.5 s pending window before Owned; the killed group completing wins Killed at once, also inside that window. A replay of an owned or killed row with a killed group shows only the killed group (ShowKilledItObj). Scoring low on the owned group ends the run as a loss with the red ScoredLow message; on the killed group it only prints. A timed row (TimeLimit, unless DisableTimeLimitForSinglePlayer offline) pins the player at time up and sends a bailing skater to the start, then ends 1 s later; untimed rows count up with no clock. Leaving the boundary repositions after OutOfBoundsTime and the run goes on. The owned / killed banner shows for its display time (3.1 / 3.3 s), then the challenge ends, plays the highlight reel and opens Continue / Retry.

**Change.** `crates/skate-core/src/challenges/hom_run.rs` (new): `HomRun` with the states, events and commands of hom_challenge in single player, the per-competitor values in `Competitor` (multiplayer: one per skater, the authority owns timers, results and teleports). `Params` holds the row values and the graph constants as overridable defaults; `restart_on_wipeout` is a switch a mod can turn off (retail on).

- The objective API for the Hall of Meat rows (b967 §5 step 1): `Wipeout.IsWipeoutOver` (82C64890: in the bail with the pose output's over flag +599; NOT RETAIL YET: its two bail-end flags +0x44 / +0x45 are not mapped) and `Reset.ResetAllObjectives` (82C6F430: every objective fully reset and inactive, `FlowCommand::ResetAll`). The shared objective feed sends `Wipeout` when the bail opens and `WipeoutOver` when it closes (the wipeout state's edges), so activation mode 4 rows (10 of the 25) arm on a bail and re-arm for the next. b967 counts 5 rows that need nothing more.
- The pose a bail closes in (b967 §5 step 2): at bail close the record's pose (P+0x3540 = the pose the last air tick matched, else none; 0x82DADC48, b880 §1.5) is latched into `Challenges::last_bail_pose` by `career_stats_host.rs`, and `ObjectiveHOM.DidSpecificBodyTweakThisFrame(i)` (82C68880) compares `i` with it. [inferred: the M+0x8B9 flag that gates the pose is read as "a pose is matched now"; when retail evaluates the rows against the close is not traced]

**Verification.** `hom_run_tests.rs`: 13 tests (setup to the run through the countdown, bail teleport and the marker cancel, the best bail, the mod switch, owned after the pending window, killed inside it, the replay rule, scored low on each group, time up with a bailing skater, single player without a limit, the boundary, retry, abandon, the DPO bar); `a_wipeout_objective_arms_on_the_bail_and_reads_wipeout_over`, `reset_all_objectives_leaves_every_objective_inactive`, `a_body_tweak_objective_reads_the_pose_the_bail_closed_in`.

**Not yet (NOT RETAIL YET).** The host (export of the Hall of Meat rows, the bail score `BailScored` from the HomScoring table, the ObjectiveHOM API, the homscoring HUD, the highlight reel and results screen), b967 §5 steps 1 to 5. Online branches (ghost restart, players-finished DPO, coop score) are not ported.
