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
  (not with the session marker modifier) writes `ChallengeSignUp { body, challenge }` (a serialisable message; the
  authority starts the run), which starts it as `SKATE_CHALLENGE` does. Without the stage, a text line shows.
- Order: the sign-up runs after the teleporter prompt and before the menu stage (`MenuStageSet`), so the stage reads
  one frame's final prompt.
- `--verify` runs: `SKATE_VERIFY_CHALLENGE=<id>` places the skater at the challenge's locator and presses for them
  once the prompt has shown 30 frames.
- Several challenges in range (research b851, checked by main in 826660F0): retail lists every challenge whose sphere
  holds the player, create-a-spot (eChallengeTypes 35) last in query order and the others before them in reverse
  order (82D29C38). The overlay then shows the Challenge state 1 with `ID_SIGNUP_MULTIPLIESIGNUP_SIGNUPHUD`
  ("Multiple Challenges", 82665DA0), and D-pad Up opens a popup (82D0A8D0): that title, an empty body (`#`), up to 4
  entries labelled by each challenge's Title id (create-a-spot: `#CreateASpot Needs A Name!`), cancelable; a pick
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
- `ChallengeHudState` (a serialisable resource: shown components, timer text and colour flag, two DPO slots,
  objective rows) is what the elements hold. challenges.rs writes it from the run's commands: `ShowHud`,
  `StartTimerHud` (shows the timer), `UpdateHud` (DPO 0 = score / target titled `ID_COMMON_TARGET`, DPO 1 = score /
  personal best titled `ID_DPO_PERSONALBEST`, objective row 0 = `ID_CHALLENGE_OBJECTIVE_BEAT_OWNSPOT` /
  `_BEAT_KILLSPOT` / `_BEAT_PERSONALSCORE`), and the new personal best central message
  (`ID_LEADERBOARD_NEW_PERSONAL_BEST`, green). The end of a run or a map change clears it.
- The timer element (82663940): the out-of-bounds countdown's remaining time rounded up, at least 0, `M:SS` from a
  minute up and bare seconds below; the colour flag drops to 0 (red) under 10 s (0x821963E4); under 10 s each new
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
  research b847), and the colour goes through 82850618 as the style, so style 2 (the movie's celebratory style).
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
(the race start, then the previous leg's last point) through the centres of its split trigger volumes (`sub_82560BC8`;
the volume point is the box centre in world space, `sub_82DD8510`, which is our exported `shape.center`). Every tick
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
a new optional field) and the completion, and sends the result to the career. Not yet: AI racers (b987 / b993),
sign-up from the map, the 3-2-1 HUD, DPO lines, leaderboard, gates and the results screen (b995 / b997; results
continue at once, as the end dialog does without its movie). A completed race leaves the career's active list:
race_shared's `complete` calls CompleteChallenge with no re-activate flag (`CALL 5 3 1`, two arguments; research
b998), as films, follow the leader, hall of meat and contests do; only own the spot (true) and tutorials
(Persistent) pass one. Test: `race_host::tests` (a disc-shaped row loads, run parameters, slot 0,
course from the split centres).

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
  count as riders, so the branch score's crowding term spreads neighbouring slots over the lines) and pins it (a
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
- NOT RETAIL YET: racers stay in the replay tier (retail racers are full skaters), so a racer starts on its line's
  first node, not on its own slot (retail RepositionAI puts it on the slot and it joins the line; race_dwtn_02 has 3
  line starts for 5 slots, so racers share starts); a racer that branches onto a side line with no set line
  starting within 4 m of its end stops there: retail's line search falls back to the nearest valid start at any
  distance (`82458968` mode 1, b972 §4) and the full skater rides over, which the replay tier cannot (desktop run
  2026-11-08: racer 3 stopped 12 s in at a race_dwtn_02 line end 41.6 m from the nearest start; racers 1, 2, 4, 5
  finished in 64.62 to 70.72 s); a racer's tricks and
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
stage's lookup is case sensitive), its buttons read the localised option texts (text natives answer `#` + the
localised text), its title comment comes from the movie's own `Math.random()` pick, and A picks Continue and completes
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

## Files

- `tools/asset_pipeline/vlt.py` (`field_bytes`), `tools/asset_pipeline/test_vlt.py`
- `tools/asset_pipeline/challenges.py` (new), `tools/asset_pipeline/test_challenges.py` (new),
  `tools/asset_pipeline/objective_lua.py` (new), `tools/asset_pipeline/test_objective_lua.py` (new)
- `tools/asset_pipeline/install.py` (the export step, after teleporters)
- `tools/asset_pipeline/map_volumes.py` (`strict`), `tools/asset_pipeline/retained.py`, `test_retained.py`
- `tools/prepare_hud.py` (the six challenge movies)
- `crates/skate-core/src/challenges/` (mod, timers, ots, ots/tests, objectives, predicate), `crates/skate-core/src/lib.rs`
- `crates/skate-game/src/challenges.rs` (new), `scoring_runtime.rs` (banked sequences), `main.rs`, `app.rs`
- `crates/skate-game/src/challenge_records.rs` (new), `crates/skate-game/src/hud_challenge.rs` (new), `scoring_hud.rs` (hosting), `hud_central.rs` (colours shared)
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
