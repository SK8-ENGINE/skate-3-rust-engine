# 43. Career mode

Status: in progress on branch `world/challenges` (with doc 42), milestone M1 "career skeleton" step 1 of 13.

## Problem

Retail Skate 3's career (board sales, milestones, unlocks, team, the career start and every challenge family) is not
in the engine. The challenge work (doc 42) runs own the spot on its own, without paying anything into a career.

## How retail does it (research, `.local/research/`)

- Checklist and roadmap: b853. Career progression runs on the same state graph and handler system as the challenges,
  one level up, from the progression bank `db.big data/db/progressionbanks/main` (1325 rows).
- Counters, conditions and pay order (b855, checked in Ghidra): board sales = owned it + killed it + bonus + set sales;
  money separately. Progression actions hold an AND list of conditions (0 board sales, 1 challenges done in a group,
  2 one challenge done, 3 never, 4 money earned, 5 money); actions are checked on events (first owned / killed, a board
  sales change, money gain), not polled. A first owned result pays the challenge's OwnedItReward, a first kill pays
  KilledItReward too; nothing pays twice.
- The career start (b868, checked), the progression graphs and their census (b861: 180 logic graphs, almost all
  straight-line), the save layout (b862, checked), team and branding (b870), unlocks (b872), the crossbar and Skate
  Profile feeds (b873 + follow-ups), presentation (b874), achievements (b881), and specs for every challenge family
  (b858 tutorials, b875 films / photos / follow the leader, b876 races / Hall of Meat, b877 contests / turn-based).
- The M1 design (b887): career logic as an engine-independent module driven by serialisable events, the progression
  graphs as a data table translated at setup (no Lua runtime), one `career.json` per player profile, unlocks recorded
  but everything left unlocked by default (an earlier decision of the user), and a `sdk.career` mod entry.

## Decisions (user, 2026-11-06)

- The crossbar keeps retail's welcome gate (the board sales bar only after `film_skpk_univ_01` and `photo_univ_05`) in
  data, plus a "skip start" switch (also for mods and as an environment flag) so the bar shows before those exist.
- Presentation the engine does not have yet (Skate 101 cards, yes / no popups, career photos) is answered at once and
  logged as NOT RETAIL YET, so progression graphs finish and pay out.
- Own the spot wins recorded before the career save existed do not pay when they are imported.

## How progression runs (research b888, checked in Ghidra)

- ArmAction (82C1BF50): an unknown or already armed action is left alone; otherwise its conditions are checked at once;
  an action that holds fires without being armed, one that does not goes to the front of the armed list.
- Only `starting_action` is armed by the engine (rows whose auto-start bool is set, not yet on the fired list); it has
  no conditions, so it fires at once. Every other action, the milestones included, is armed by progression graphs.
- Fired actions leave the armed list after each check walk.
- One graph runs at a time (a FIFO queue); a graph ends when it enters the state named `complete` (not on a state
  without handlers); the next queued graph starts on the next manager update.

## Change: the career core (`crates/skate-core/src/career/mod.rs`)

Counters (board sales = the four sales counters; money and money earned, spending clamps at 0), the six condition
types AND-ed, actions, and the tracker: `arm` as ArmAction above, `check` per trigger (a challenge's first result,
sales changed, money changed) checking only the actions that watch it, firing in list order. `Career::challenge_result`
pays in retail's order (827CDDD8): with no earlier result the owned it reward, a kill not killed before the killed it
reward too, then money checks, board sales checks, the status upgrade and on a first result the challenge checks; it
returns effects (paid, status changed, action fired) for the game to log, save and grant. Tests in the module.

## Change: the career save (`crates/skate-data/src/career_save.rs`)

`settings/profiles/<player>/career.json` (`skate3rust.career` v1): challenge statuses (completion, high score,
discovery), the counters, the armed / fired action lists and the career flags (absent = unset, so a flag never set is
not `false`: retail tests `== false`, research b889). Every level keeps keys it does not know. The first load imports
`settings/challenges.json` once as statuses only: wins from before the career save pay nothing. Tests in the module.

## Change: the graph runner (`crates/skate-core/src/career/graph.rs`)

Graphs as data (states by name, handlers by message, ops), run one at a time from a FIFO queue: a graph starts at
`initial` with `stateenter`; a handler's ops run in order and its state change (the last op) applies after them; a
graph ends when it enters `complete`, after that state's stateenter ops; the next queued graph starts on the next
update; a graph without states never ends (blocks the queue, as in retail). Career flags are three-state; the school
exit's `pick` / `$picked`; a career photo reply must carry the request id the graph stored. Everything the game must do
comes out as a `Request` (arm action, unlock reward, activate challenge, presentation, teleport, graph started /
ended); presentation is answered by the host with the reply message (M1 answers at once, logged NOT RETAIL YET).

## Change: the unlock store and the career loop (`career/unlocks.rs`, `career/mod.rs`, skate-data `progression.rs`)

- Unlocks: grants by retail kind (0..12) and name with a "new" flag, the two reward summary lists (big / others), and
  `unlock_all` on by default, so every query answers unlocked until the user turns gating on.
- `Career::start` arms the auto-start actions not yet fired (only `starting_action`); `drive` starts the graphs of
  fired actions and routes a graph's ArmAction back through the career (more fired actions queue their graphs);
  `update` starts the next queued graph; `deliver` passes a presentation reply. Everything else comes out as requests.
- `skate_data::progression::parse` loads `progression.json` into these types; an unknown op or condition is an error.
- Checked on the user's own export (ignored tests with `SKATE_PROGRESSION_JSON`): it loads completely (204 actions,
  53 groups, 181 graphs, all with `complete`), and a new career runs retail's `starting_stategraph`, which activates
  `tut_sksc_01` and `ftl_univ_02` (research b861); `starting_action` fires once per career.
- Reward screens (research b893): the reward summary (FE 64) opens when the challenge's graph has finished; A goes on
  to the milestone cards (65) then teammate cards (66); the summary exit and the card chain clear the lists; the
  deferred milestone scenes play after that.

## Change: the career in play (`crates/skate-game/src/career.rs`)

With the Challenges opt-in, the game loads `private/progression.json` and `settings/profiles/local/career.json` (a new
one imports `settings/challenges.json` once, statuses only), runs `Career::start`, and takes each finished challenge
run from `challenges.rs` (`ChallengeResult` message): pay order, fired actions, graphs. Presentation requests are
answered at once and logged NOT RETAIL YET; every request and effect is logged as `SKATE_CAREER ...`; the file saves on
a short-lived thread after each change. Compiles; its tests and a verify run are still to do.

## Change: the progression export (`tools/asset_pipeline/progression.py`)

Setup writes `assets/private/progression.json` (`skate3rust.progression` v1) after the challenges export:
- `groups`: each progression group's challenge ids (the class-typed challenges RefSpec array).
- `actions`: per action its condition list (type, threshold, the referenced group or challenge id), the graph it
  starts and whether it stays on the fired list (only `starting_action`).
- `rewards`: per reward row its unlock kind (inherited from the category parent rows), unlock name, title and image.
- `challenge_rewards`: per challenge the owned it and killed it money, board sales and reward ids, plus the online bonus
  sales (never paid offline), family fields inherited.
Names stay strings (stable ids for the engine and mods); references into the challenge banks resolve through the
challenge keys. The graphs follow in step 7 (translated handlers).

DLC progression banks (2026-11-07, research b912): retail mounts one progression bank per pack (`progressionbanks/
dlc_<pack>` beside the disc's `main`); the export used to read only `main`. It now reads every bank, the disc's `main`
first, then each pack's, translates each bank's graphs against its own `.bin` and merges them (the first copy of a key
wins). `banks` names each bank's source and `from_bank` lists the keys a pack added (the engine ignores both). Install
now copies a pack's BIG when it holds a progression bank too, and no longer only when some pack has map jobs. With the
user's four packs: `main` 204 actions / 181 graphs, `dlc_mmcn` (Maloof) 16 / 13, `dlc_play` 22 / 19, `dlc_dwgh` (Danny
Way) 35 / 30, `dlc_player1` 7 / 4; 231 keys from packs. A pack's starting action (`mmcn_starting_action`, record
fired, no conditions) arms itself like the disc's `starting_action`, so the Maloof chain (street contest, then both
photos, then both lots) runs with no engine change. [data]
- New graph call: `Discovery.SetDiscoveryStatus(challenge, Discovery.eDiscoveryStatus.<status>)` (only in the Danny Way
  bank, `ots_dwgh_01`) translates to `set_discovery`; the career writes the challenge's discovery record. [data]
- Check tools: `.local/tools/progression-export-check.py` (export with the packs into a work folder) and
  `progression-call-scan.py` (lists every handler call the translator cannot translate).

## Change: the graph translator (`tools/asset_pipeline/progression_lua.py`)

Each progression handler is a compiled Lua 5.1 chunk (little-endian, 4-byte floats; research b889). The translator
walks its bytecode symbolically (constants, globals, field chains, call results) and turns it into ops: calls become
`arm_action`, `unlock_reward`, `activate_challenge`, `change_state` (with the Defer / Execute event mode) and the rest of
the b889 schema; writes into the shared flags table become `set_flag`; an `EQ` + `JMP` becomes a structured `if` with
`flag_false`, `reply_matches` or `completion_is`; the school exit's ordered flag chain becomes nested ifs with `pick`
ops, a `picked_none` test and `teleport_to_challenge` / `auto_sign_up` of `$picked`. Any other call or construct fails
the export (so a new bank or DLC never gives a silently wrong graph). The export writes `graphs`: per graph its states
(by name) with their presentation events (type name, two string args, mask; research b892) and handlers by
message. Tests on hand-built bytecode
(`test_progression_lua.py`, no retail bytes).

## Change: milestones, crossbar facts and the Skate Profile page (step 10; design `.local/research/b895-career-steps-10-12-design.md`)

- Export: each action's `parent`, `killable` (challenges with a non-empty killed it list or race type 0x13) and a
  `career` block (`milestone_parent`, `crossbar_gates`). The loader reads them into `Progression`.
- `career/milestones.rs`: the milestones are the actions whose parent chain reaches `milestone_actions`, sorted by
  their first board sales threshold. Crossbar index (827CD728): -2 / -1 while a gate challenge has no status, else
  count - pending - still armed. `facts()` gives GetProgressionData's six numbers, which `career.rs` writes into the
  crossbar's progression facts. `dots()` gives GetMilestoneDotData.
- `Career.pending` counts MilestoneEarned / UnlockTeammate requests (retail's reward summary +0x04 / +0x18) and saves
  in `career.json` `rewards`. NOT RETAIL YET: without the reward summary and card screens, the earned rewards count
  as presented at the end of the frame (logged `SKATE_CAREER NOT RETAIL YET summary/cards`), so the index moves then.
  Between photo_univ_05 done and its graph arming the milestones, the index reads the full count (retail too).
- Skate Profile: the summary rows (class progression_summary_group) are read at runtime from the installed
  collections (`skate-data/career_summary.rs`; no setup change). `allmilestones` / `allchallenges` are the overall
  bars and the parents of the list rows. `career/profile.rs` builds the 8-slot milestone rows (locked while above the
  goal) and the 6-slot challenge rows (members counted per group). The menu stage layers mods (`sdk.menus.profile`)
  on top of this page. The challenge bar now goes to the movie as [title, icon, total, current] (the movie's order;
  before, a mod-set challenges bar showed "current / current").
- Unlocks: the career holds the unlock store. Challenge payout reward ids and the graphs' UnlockProgressionReward
  grant through the reward rows (kind, name from `progression.json` `rewards`) and fill the reward summary lists,
  which the end-of-frame stub clears. Saved in `career.json` `unlocks` ({kind, name, new}); `unlock_all` stays the
  default (on) until gating is switched on. Mods see them in the snapshot and the `career_unlock` event.
- Unlock-gated summary rows (Ghidra 82503AE0 / 82503D50 / 82503BD8): a tUnlock field stores {kind, FNV-1 32 of
  the unlock name} (multiply then xor; the disc's world rows are FNV-1 of `megapark`, `startpark`, `downtownpark`,
  `industrialpark`). The row shows once that pair was granted; retail's unlock-all bypass (+0x24) is off for players,
  so ours does not open these rows either. The disc's only gated row (ID_WORLD_BLACKBOXPARK, kind 0, hash
  0x00009AAD, no known name) stays hidden as in retail. The DLC packs bring their own summary rows (Danny Way,
  Maloof Money Cup, Art Gallery, Sanitarium; no unlock field), so they show exactly when the DLC is installed.
- Checked against a capture of the recomp's Skate Profile (laptop, finished career, 2026-11-06): the untitled rows are
  not shown; Career Progression / Challenge Progression are only the two overall bars, not list rows (so ours leaves
  the overall rows out of the lists); the list order is the rows' order field (DLC Maloof sits between Hall of Meat
  and Own The Lot, After Dark between Races and skate.School, as the field says); the milestone icons match the
  three flags. Not ported yet: DLC rows the player does not own show dimmed with 0 in retail.

## Change: the career loads off the main thread (`career.rs` `read` / `apply`)

`progression.json`, the career and achievement files and the ~10 MB stock collections are read and parsed on a
background thread when the career first runs; the main thread only applies the result (import, adopt, clean-up,
defaults, load replay, start). Challenge results that arrive meanwhile wait and are applied after the load.

## Change: team, brand tiers, teammates and achievements (step 11)

- `career/team.rs`: five members (0 = the player) and the brand tier set. UnlockTier is idempotent; a tier query
  compares the rows' bit (field 0x4632B6D0D476D3AB, the bit TeamBrand.UnlockTier sets, b870), so tiers that share a bit
  share an unlock, as in retail. UnlockTeammate: the first member 1..4 not in yet joins, its
  `040N_team_photo_teammate_N` tier is set and the pending teammate count goes up; nothing at five. Default-unlocked
  tiers are set on every load [inferred b870]. Saved in `career.json` `team` as tier ids.
- `career/achievements.rs`: a grant needs an existing, enabled row (class 9CB9012080F727B9: platform id, enabled
  gate) and happens once. Sites: the GrantAchievement op, the GrantAchievement presentation event, and the load replay
  (827CE7B0): the actions with one board sales condition whose graph grants an achievement, granted at load once
  their threshold is reached. Kept in `settings/profiles/local/achievements.json` (`skate3rust.achievements` v1),
  separate from the career so a new career keeps them.
- Graph states now carry their presentation events (type, args, mask). Execute plays them at once; Defer queues them
  in the career and they play when the presentation releases (the end-of-frame stub). Only GrantAchievement does
  anything yet; the other event types are logged NOT RETAIL YET.
- Tier and achievement rows are read at runtime from the installed collections, keyed by the names the graphs use
  (the row keys are stored hashed), so no names.txt or setup change is needed.

## Change: the mod entry `sdk.career` (step 12)

- Snapshot `career` (counters, milestone index / count / start / goal / thresholds / pending, armed and fired
  actions, flags, team, achievements, running graph and queue) and commands `career_add_sales`, `career_add_money`,
  `career_arm_action`, `career_set_flag`, `career_grant_achievement` (`crates/skate-game/src/modding/career.rs`,
  capability `career` 1, `sdk/ENGINE_API.md` "Career").
- Mod board sales go into the bonus counter (online only in retail, 0 offline), so they count in board sales and
  run the same checks as a challenge payout; money runs the money checks.
- When a mod stops, the career changes that work without it stay (board sales, money, retail actions and
  achievements: the commands only accept ids from the game data); what depends on the mod goes: its flags
  (namespaced `<mod id>:<name>`) at once, and at load any save entry naming an action or tier the loaded data does
  not have (logged `SKATE_CAREER dropped unknown save entries`). User decision 2026-11-06.
- Events go to the mods through `on_event` (newest 64 kept).
- Also `career_grant_unlock` (retail kind as `category`, since `kind` is the command tag), `career_grant_reward`
  (a reward row: unlock plus summary list) and `career_skip_start`.
- Skip start (b887 U1, user ok 2026-11-06): the crossbar shows its welcome panel until `film_skpk_univ_01` and
  `photo_univ_05` have a status, and M1 has no way to play them. Skip start arms the actions that wait for each gate
  (the start chain's graphs normally arm them), then completes the gate as owned through the normal result path (pay, checks, graphs: photo_univ_05's graph arms the milestones), as if the
  player had done the start chain. `sdk.career.skip_start()` or `SKATE_CAREER_SKIP_START=1` at load.
- Dev mod `mods/career-test` (`dev-career-test`, off by default; `SKATE3_MODS_ENABLE=dev-career-test`): skips the
  start, adds 15,000 board sales and logs `CAREER_TEST` lines (milestone facts before / after, career events).
- Mod data: a mod ships `progression/*.json` in the export's schema with rows by id (add a challenge to a group,
  change an action's conditions or graph, add rewards or graphs, change reward amounts). Objects merge key by key,
  other values replace, `null` removes (`skate-data/progression.rs` `merge`). The running mods' overlays apply in mod
  id order over the setup export and follow the mod set (synced with the audio overlays in `modding/mod.rs`); an
  overlay that breaks the data is logged and left out. When a mod stops its overlay goes and the data rebuilds; save
  entries that only existed through it (armed / fired actions, queued or running graphs, tiers) are dropped.
- Challenge activation: the career keeps the activated set (graphs' ActivateChallenge / ActivateGroup minus
  DeactivateChallenge; `career.json` `active`) and, while gating is on (`unlock_all` off), only those challenges can
  be signed up for (`challenges.rs` `available`). Gating stays off by default, so every challenge is
  available as before. Mods: `career_activate_challenge` / `career_activate_group`.
- Activation follows retail Activate(key, force 0) (82C1D7D0, research b846 / b908): a key already active or already
  completed is skipped (`Career::activate`). [code]
- Discovery activates (research b910, verified by main: ctor 8255B290 stores the career object at +0x6180, handler
  8255BD38 calls its vt+0x14 = Activate): riding into a challenge's boundary marks it discovered and activates it
  (`ChallengeDiscovered`, `career.rs`), so discovery no longer waits for the gate; only the mod switch hides a
  challenge from discovery. [code]
- Completion and the active list (research b918, verified by main: 8283FD00 calls career vt+0x14 with r5 = 1): an
  owned / killed result always takes the key off the active list; CompleteChallenge's flag puts it straight back
  (forced). Own the spot passes it on every result, tutorials pass their row's Persistent (true only on the park
  tutorials), every other family passes nothing. Ours: `Career::settle_active` after the result, with
  `ChallengeResult::reactivate` (own the spot true; tutorials false until Persistent is exported, right for the 12
  school tutorials; skip start false). [code + data]

## Verification

- `python -m unittest tools.asset_pipeline.test_progression tools.asset_pipeline.test_progression_lua`: OK.
- Local run against the user's disc (counts only, nothing kept): 53 groups, 204 actions, 222 rewards; condition types
  108 one challenge done, 79 group done, 33 board sales, 2 money (matches b855); 179 actions start a graph; only
  `starting_action` keeps its fired flag; 495 challenges carry rewards (`ots_dwtn_01`: 10,000 sales owned, 2,500 killed,
  reward `skatepark_unlock_ots_dwtn_01`). One condition references a challenge outside the banks (`tut_univ_01`,
  cut content per b861). Graphs: all 248 handler chunks translate with no failure; 181 graphs, each starting at
  `initial` and each with `complete`, 386 states, 246 handlers; the two school exits pick from 6 and 4 challenges.

## Open questions

- What sets the career manager's auto-start flag (new career only or every load); harmless either way (b888).
- Whether any progression graph lacks a `complete` state (it would block the queue, retail too): a setup export check.
