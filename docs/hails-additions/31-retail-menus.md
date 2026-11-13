# 31: Retail menus: menu data, hook registry, mod API (milestone 1) and settings value model (milestone 2)

Branch: `ui/retail-menus` (from `main` b3c96793). Own upstream PR when ready (not part of the audio or living-world
PRs). This milestone adds data, hooks and the mod API only; nothing is drawn yet and the current overlay menus
(`graphics_menu.rs`, `modding/menu.rs`) are unchanged.

## Problem

The engine's pause and settings menus are hand-built (`graphics_menu.rs` sections MAPS / SKATER / GRAPHICS /
MULTIPLAYER / EXTRAS). They do not look or behave like Skate 3's, and they have no structure that features or mods
can plug into: every new feature edits menu code. A real accident showed the cost: the FPS limit was changed by an
unintended press in the overlay. The goal of the menus work is retail menus (layout, navigation, input handling,
sounds) built on the retail data, moddable from the start. Milestone 1 is the foundation: the retail menu
structure as data, a registry with hooks for every entry, and the mod API over it.

## What retail does

Reference: the TU3 (title update 3) build through skate3recomp (by @mchughalex, built on the rexglue SDK) read statically (no game
run), the front-end movies in `data/big/fedata.big` read with upstream's APT (EA's Flash-based menu movie format) tools (`tools/vendor/skate3_ui`, from
chazmm's APT/HUD port 1c768a3e). Addresses are TU3 and only used to verify the extractor; no game code or data is
copied. Full notes: a local front-end menus research note.

**The movies ask the game for data.** The pause menu (`screens/main/core_menu.apt`, the "crossbar") and the
settings screen (`screens/options/options.apt`) are AS2 (ActionScript 2) movies; they read the menu contents through host objects
(`CoreMenu`: GetNumMenus / GetOptionName / IsItemEnabled ...) and natives (`GameSettings_GetNumOptions`,
`OnMenuSelect`, `OnMenuGetItemEnabled`, ...; registrar `825A3070`). The contents are C++ tables:

- **Game Settings rows** `0x83026EC8`: 36 x (string id, widget type `option` / `slider` / `selector`); rows 0 and 23
  are blank. **Screens** `0x83026FE8`, right after: 13 x 16 row indices, 0-terminated (1 = main: Difficulty, Audio,
  Video, Controls, Online, Skate Feed; 3 = Audio: SFX / Dialog / Music volume, SFX pack; 4..8 = Video variants; 9
  Controls; 10 Online; 11 Skate Feed; 12 Audio without the SFX pack, unused).
- **Crossbar categories** `0x8302751C`: 12-byte records (name, label id, icon): Multiplayer, Options, SinglePlayer,
  Create, Learn. **Items** `0x83027558`: 20-byte records (name, label id, icon label, sub-option kind or -1, helper
  text id), 49 items from ReplayEditor (id 0) to ExitPartyPlay (id 48). Item ids are positions in this table.
- **Apt key table** `0x83027BF0`: 56 x 20-byte records (input action, name, Apt key code, released flag, kind):
  AptStart 0x12D, AptNext 0x12E, AptPrevious 0x12F, AptUp / AptDown / AptLeft / AptRight 0xE / 0xF / 1 / 2 and the
  `*Released` twins.
- **Per-mode menus** are static data reached through the crossbar mode classes. Each class's vtable (e.g. Career
  `0x82303D54`) holds, in order: destructor, a tab accessor `GetMenu(i)` (`lis; rlwinm r4,2; addi; lwzx; blr`, e.g.
  `825E8368` -> `0x822059E4`), a row accessor `GetItem(tab, slot)` (`825E8380`, rows `0x82205A68`, stride 8 via
  `rlwinm r4,3`; the park variants use `r4*8 + r4` = 9 slots), the item-kind getter, the tab count (`li r3,5`), the
  row counter (skips Project10 until unlocked, `8261F950`), IsItemEnabled, a constant, the mode title getter
  (`8261F940` -> `ID_CROSSBAR_SINGLE_PLAYER_MODE_TITLE`) and the select hook. Nine classes have static tables:
  Career, Career in a park, Free Play, Free Play in a park, a restricted free-skate menu (Main = ChallengeMap only),
  the Skate challenge, two lobby modes and the Party Play menu. Career Main = ChallengeMap, MyCareerTeam,
  SkateProfileOffline, SkateWith, SkateFeed, SoloFreeskate, PartyPlay (matches the retail screenshot row for row).
- **Rules** (from the code): the settings row count stops at the SFX pack row with <= 1 SFX pack
  (`GameSettings_GetNumOptions` 825A6F70) and IsItemEnabled (`8260D648`) greys it, greys Difficulty in online modes
  (GameMode 4 / 5) and Skate Feed settings when unavailable (`8245A2D0`). In a park (`8261FB18`) SkateReel and
  SkateWith are greyed, SavePark needs a park-state flag, RatePark / FlagSkatePark only in a community park. The park
  select hook (`8261F9D8`) asks `ID_SKATEPARK_DIRTY_TITLE` / `_DESC` (Yes / No) before ChallengeMap /
  OnlineChallengeMap leave a dirty park. Settings select (`8260D290`) links rows to screens, Video by front-end
  state, mode and session (screen 8 / 7 / 6 / 5 / 4). Retail's own greyed look is the row state `disabled` in
  core_menu's row sprite.

## Change

### 1. Extraction at setup (`skate_data::menu_tables`)
`skate3rust --extract-menu-tables <default.xex> <menu-tables.json>` (like `--extract-ocean-pca`) unpacks the user's
own executable with `XexImage` and writes `assets/private/menu-tables.json`; setup runs it in the environment stage
(`tools/asset_pipeline/menu_tables.py`, `asset_exports.py`; failures become an optional-content note). Every table is
found by content or code pattern, never by address, so any build works:

- settings rows: the pointer to the string `ID_GAMESETTINGS_SFXVOLUME` whose neighbour points to `slider`; the run of
  (string, widget) pairs around it; the screens follow while every block holds valid row indices;
- crossbar items: the pointer to `ReplayEditor` in a record of the item shape (three strings, a small kind, a
  string); categories are the 12-byte `ID_CROSSBAR_*` records right before;
- Apt keys: the pointer to `AptStart` in a record of the key shape;
- per-mode menus: every pair of adjacent vtable slots where a small evaluator (lis / addi / ori / rlwinm / add /
  mulli / lwzx / lwz / blr) shows the first function reads `table[r4]` and the second `table[r4 * n + r5]`; the
  following slots give the tab count (`li r3,n; blr`) and the title (a two-instruction getter returning a string).
  Item and category ids become names. Mode keys: `Career` / `CareerPark` (single-player title, 8 / 9 slots),
  `FreePlay` / `FreePlayPark` (offline free-skate title; FreePlay has ResumeCareer), `FreeSkateRestricted` (same
  title, no ResumeCareer), others by title (`SKATE_CHALLENGE`, `TEAM_VS_TEAM_LOBBY`, `RANKED_LOBBY`, `PartyPlay`).
  Retail has no class names for these menus, so the keys are ours, derived from the retail title and slot count.

The JSON (`schema` 1) also keeps the source image hash and the table addresses for diagnostics.

**The disc build differs from TU3**, which is why nothing is fixed by address or index: the user's disc
`default.xex` has 47 crossbar items (TU3 added PartyPlay and ExitPartyPlay), 35 settings rows (no
`ID_GAMESETTINGS_SKATEFEED_STATUS_MESSAGE`, so the Skate Feed settings row is index 34, not 35), its categories sit
right after the items instead of before them, and every table is elsewhere (settings rows `0x82FCB168`, items
`0x82FCB7A8`, categories `0x82FCBB54`, keys `0x82FCBE50`, Career vtable `0x822FE764`). Its per-mode menus are the
TU3 ones without the Party Play entries and without the Party Play menu. The registry keys settings rows by string
id, so screens stay correct in both builds.

### 2. Registry (`skate_core::menus`, resource `retail_menus::RetailMenus`)
Engine-independent registry over `MenuData` (categories, items, settings rows, screens, modes), built from the JSON
(`MenuTables::to_menu_data`; settings rows keyed by string id, blank rows `GAMESETTINGS_ROW_<n>`). Keys are the retail
internal names (`GameSettings`, `SkateFeed`, `SinglePlayer`, `ID_GAMESETTINGS_SFXVOLUME`).

- Hooks per entry: `visible` and `enabled` (data `Rule`s over published facts and the mode, or native predicates),
  `on_select` / `on_highlight` handlers, select interceptors (confirmation popups), value get/set bindings. Every
  hook list is a stack by owner (`Retail`, `Engine(name)`, `Mod(id)`); the last registered wins and removing an
  owner restores the one below.
- **Greyed until handled:** an entry is enabled only when a handler (or value binding) exists and its enabled rule
  holds; a settings link is enabled when its target screen has an enabled row. So every retail item exists from the
  start, greyed, and a feature enables it by registering a handler, with no menu code changes. Online / Xbox LIVE
  items stay greyed placeholders until the custom online feature registers.
- Retail rules registered as `Owner::Retail` data: SFX pack hidden and greyed at <= 1 pack (fact `sfx_packs`),
  Project10 hidden until `project10_unlocked`, Difficulty greyed when `game_mode` is 4 / 5, Skate Feed settings by
  `skate_feed_available`, park rules (SkateReel / SkateWith off in `CareerPark` / `FreePlayPark`, SavePark by
  `park_saveable`, RatePark / FlagSkatePark by `park_community`), the dirty-park confirmation on ChallengeMap /
  OnlineChallengeMap, and the settings links incl. the Video screen choice (`retail_link`).
- Actions are values with stable ids (`MenuAction::Select / Confirm / Highlight / SetValue`, entry kind + id, plus the
  mode): `dispatch` returns what the host does next (`Confirm(popup)`, `Handled`, `Forward` to a mod,
  `OpenScreen`, `Ignored`, `Declined`), so a menu UI or a remote peer can replay the same actions (multiplayer-ready,
  no networking).
- Structural edits (`Edit::Category / Item / Setting / Hide`) are owner-tagged layers over the retail base: add,
  hide (per mode), reorder, relabel, change icon. Removing an owner rebuilds the structure from the base plus the
  remaining layers, so disabling a mod reverts exactly its changes.

### 3. Mod API (`sdk.menus`, capability `retail_menus` = 1)
`sdk.menus.category / item / setting(id, options)` (add or change: label, retail `icon` label or mod `image`,
help, tab `category`, `modes`, `position`, `widget`, `screen`), `hide(kind, id, modes)`, `handle(kind, id, {select,
highlight, value, enabled, visible, confirm})`, `unhandle`, `set_value`, `value`, `info`. New entries must be named
`<mod id>.<name>`; 64 entries per mod. Handled entries send `on_event {name = "menu_select" | "menu_highlight" |
"menu_value", entry, id, mode, value}`. Rules from Lua: `true` / `false` or `{fact = "sfx_packs", gt = 1}`,
`{modes = {...}}`, `{all = {...}}`, `{any = {...}}`, `{["not"] = ...}`. `sdk.snapshot.menus` holds the mode, whether
the retail tables loaded and every bound value. Everything a mod did is removed when it stops, fails or reloads.
Mod images are a designed field only; drawing them comes with the menus (milestone 5).

## Milestone 2: settings value model (2026-10-08)

### Problem
Milestone 1 had value get/set hooks but no rule for how a row's value changes, and no engine setting was bound, so
every value row was greyed.

### What retail does
All from the TU3 settings input handler `sub_8260C258` (events 14 = Left, 15 = Right), GetIntegerValue
`sub_8260D7E8` and GetStringValue `sub_8260D968`; values live in the settings object `[[0x83067060] + 28]`.
- [code] Volume sliders (rows SFX / Dialog / Music, offsets +128 / +132 / +136, f32):
  `v = clamp(v + dir * 0.1, 0.0, 1.0)` with `fmadds` (step 0.1 at `0x820641A8`, min 0.0 at `0x82165A10`, max 1.0 at
  `0x8231A844`, Left -1.0 at `0x8216DEE0`, Right uses the 1.0 at `0x8231A844`), then `v < 0.05` (`0x82165A00`) -> 0.0.
  The slider shows `floor(v * 10.0 + 0.5)` bars (10.0 at `0x821963E4`, 0.5 at `0x8209975C`).
- [code] Selectors toggle `x = (x == 0)` on either direction; Left and Right only differ in their UI sound. Labels
  (sub_8260D968, strings at `0x82203990 - 668 ..`): Camera Angle +188 0 = `ID_GAMESETTINGS_CAMERA_LOW`, 1 = `_HIGH`
  (other values `#ERROR!!`); offboard Y / X axis +192 / +193 `_AXIS_NORMAL` / `_AXIS_INVERTED`; Subtitles +157,
  Minimap +158, HUD +164 (applied by `sub_824AD2F0` / `sub_825DA2A0`), Camera manual +165, Vibration +163
  (`sub_8260DF40`, plays a test rumble when switched on), Transparency +195, Skate Feed +196..+200:
  `ID_COMMON_OFF` / `_ON`; Units +168 `_UNITS_METRIC` / `_IMPERIAL` (also mirrored to `[0x830CFE34] + 4`).
- [code] Play Mode (+180, `sub_8260DE60`): `v + dir`, above 2 -> 0, below 0 -> 2; applied at once by
  `sub_827D8450` unless `sub_82743FC0` (online session). Labels `sub_825E7648`: 0..4 = `ID_GAMESETTINGS_DIFFICULTY_`
  `EASY` / `NORMAL` / `HARDCORE` / `MOTORIZED` / `TEST`, nothing above 4.
- [code] HOM mode (+184): `v + dir`, 2 or more -> 0, below 0 -> 1 (two values, `ID_COMMON_OFF` / `_ON`).
- [code] Not value rules: SFX pack (`sub_82486870` picks the next pack id), Auto sign-in (`sub_8260E068`, a popup);
  VSync and Sixaxis have no case (Left / Right do nothing). Every change refreshes all rows (`vtbl+20(item, 50)`).
- [code] Volume routing: the MixMap Master controller's step `sub_824D5160` (runs every audio pass, so a change is
  heard on the next pass) reads the three floats from the settings object and writes its own inputs through
  `[controller + 12]->vtbl+8(input, value)` with `value = trunc(v * 32767.0)` (`fmuls` by 32767.0 at `0x821747FC`,
  `fctiwz`), clamped to 0..32767: Master.in1 = Music (+136), in2 = SFX (+128), in3 = Dialog (+132). The MixMap data
  then scales the buses (mixmap-spec section 6.5: in1 -> Music, in2 -> nearly every SFX sum, in3 -> Announcer, NIS
  and CameraMan speech). Ambience and emitters hang on Master.in4, which is not an option volume (same function, from
  a game object's float, 32767 in free skate). Music +136 is also a gate elsewhere: `sub_8249A7E8` and
  `sub_824CF350` only start / advance a music track when it is above 0.0, `sub_8249C078` tests it for 0.0;
  `sub_825AD1F8` writes +136 from a system call result (`sub_82E5F430`).

### Change
- `skate_core::menu_values` (new): `ValueRule` {Slider, Toggle, Cycle} with the arithmetic above (f32,
  `mul_add` like `fmadds`, deterministic), `Display` (bars or a label id), `Direction` (serialisable by name), and
  `retail_rule(id)`: the retail rules keyed by string id (disc and TU3 tables agree), so ranges and steps are data.
- Registry: retail rules installed as `Owner::Retail`; `set_rule` stacks an owner's rule over it (last wins);
  `MenuAction::Step { entry, direction }` turns Left / Right into a `SetValue` through the rule in force;
  `rule`, `display`, `owner_value`, `set_owner_value`. Removing an owner drops its rules too.
- Engine bindings (`retail_menus.rs`, `Owner::Engine`), which enable these rows: SFX / Dialog / Music volume <->
  `game_audio::RetailVolumes` (0..1 floats, default 1.0, saved in `settings/audio.json` as `retail_volumes`), which
  the native pass (`native::mixmap_frame`) writes to Master.in2 / in3 / in1 exactly as `sub_824D5160` (it wrote
  32767 to all four before); our own Master / Ambience / Effects percent settings are separate engine settings and
  no longer tied to a retail row. Camera Angle <-> `CameraAngleSettings.selected` (0 Low / 1 High, saved),
  Play Mode <-> difficulty (physics mode, `Config`, `settings/gameplay.json`). A sync system applies a row change to
  the engine and saves it, and takes changes made elsewhere (current menu, settings files) into the row without an
  event. Motorized and Custom stay settings-file values (shown as their index label; the retail rule steps back into
  Easy..Hardcore). Engine-only settings stay in the settings files (decision above).
- Mods: `sdk.menus.handle('setting', id, {value = ..., rule = {kind = 'slider' | 'toggle' | 'cycle', ...}})` binds a
  value and / or overrides the rule over the engine binding; `set_value` and Left / Right reach the mod layer
  (`menu_value` event); disable / reload / failure removes both and the engine value shows again.
- Mod `set_value` never writes the user's real setting (follow-up 2026-10-08). On a row whose value in force is not
  the mod's own (an engine binding, another mod's value) the value goes into the mod's own value layer on top of the
  owner stack, exactly as `handle('setting', id, {value = ...})` does (`MenuRegistry::value_owner` tells which layer
  is in force). Last writer wins: a mod writing again moves its layer back on top. While a mod layer is on top of an
  engine-bound row, the sync system applies its value at runtime without saving: SFX / Dialog / Music through
  `AudioSettings` runtime volume overrides on the Master inputs (`settings/audio.json` keeps the user's volumes),
  Camera Angle as the camera's forced angle (owner `menus`; the player's `selected` angle is untouched), Play Mode
  as the physics mode and `Config` difficulty without writing `gameplay.json`. When the layer goes (disable, reload,
  failure) the next layer down is applied again, ending with the user's own setting. A mod's `handle` value
  override on an engine row is applied the same way (before, it was only shown). On a row whose top layer is the
  mod's own, `set_value` acts like a menu change (`menu_value` event to the mod), as before.

### Verification
- `cargo test -p skate-core menu_values`: slider steps, clamp, snap below 0.05, exact zero from 0.1, bars rounding;
  toggles on both directions and any non-zero; Play Mode and HOM wrap, labels incl. out-of-range; undecoded rows have
  no rule.
- `cargo test -p skate-core menus`: Step through the retail rule on an engine binding (0.75 -> 0.85, Hardcore ->
  Easy), engine-side change without an event, mod rule + value override and exact revert to the retail rule and the
  engine value.
- `cargo test -p skate-mods menus`: rule specs parse and validate.
- `cargo test -p skate-game mod_set_value_layers_over_engine_rows_and_reverts`: a mod's `set_value` on the SFX volume
  and Camera Angle rows is shown and applied (Master.in2, forced camera angle) while the user's volume and camera
  choice stay; a second mod on top wins, the first writing again wins back; removing each mod reverts only its own
  layer; after both, the engine value is back exactly, `audio.json` is byte-identical and `camera.json` was never
  written.
- `cargo test -p skate-game retail_volumes volume_rows`: default 1.0 -> 32767 on in1..3; 0.9 / 0.5 / 0.0 -> 29490 /
  16383 / 0 (truncation), clamp and NaN, clamping on load; each row stepped Left reaches only its own Master input
  (SFX in2, Dialog in3, Music in1), is saved, and an engine-side change shows in the row.

### Open
- Subtitles, Minimap, HUD, offboard axes, Camera manual, Vibration, Units, Transparency, HOM
  mode, Skate Feed rows: rules ported, no engine setting yet, so they stay greyed until a feature binds them.
- Music volume as a gate (no track started at 0.0, `sub_8249A7E8` / `sub_824CF350`): needs the music player.
- Retail's default volumes (settings constructor, inside the profile save) not read; 1.0 matches the recomp's
  free-skate capture (Master.in1..3 = 32767).
- SFX pack (`sub_82486870`) and Auto sign-in (`sub_8260E068`) rules not decoded.
- Retail defaults of the settings object (its constructor) not read; our bound settings keep their own defaults.
  Update 2026-10-11: read [trace] instead: the recomp booted through the demo path without a signed-in profile,
  `SKATE3_WATCH` on `0x83067060 +28 @ +128` (hex64) and `+192` (hex16), files in a local recomp trace folder
  (the zeroing function `sub_82DE3F38` clears the object first; the values come from later code, not decoded).
  Values: volumes 1.0, Subtitles 0, Minimap 1, Vibration 1, HUD 1, Camera manual 0, Units 0, Play Mode 1, HOM 0,
  Camera Angle 0, axes 0 / 0, Transparency 0, the five Skate Feed toggles 1. `skate_core::menu_values::retail_default`
  holds them; `MenuRegistry::value` falls back to it when a row has no value layer, so rows without an engine
  setting show retail's value instead of "ERROR!!" (they stay greyed: a default is not a binding). Video Settings
  before: Subtitles / Minimap / Cameraman Mode / HUD read "ERROR!!"; after: Off / On / Off / On. Test
  `every_rule_row_has_a_retail_default_that_displays`. The recomp is not retail: the values are its boot state
  with no save loaded.
- The current overlay (`graphics_menu.rs`) keeps its own difficulty copy; it is replaced in milestone 5.
- While a mod holds Play Mode, a difficulty change saved from the current overlay saves that overlay's choice; the
  mod's value itself is never saved. A mod already holding the camera through `sdk.camera` keeps it (the menus
  hold is refused with a warning).

## Milestone 3a: APT VM opcodes for the menu movies (2026-10-08)

### Problem
The 16 retail front-end movies (core_menu, options, highlight_selector / option / slider, screenmanager,
screentransitionmanager, menu_picker, tabs, button_item2, menu_part_text_hilite, panel, scrollbar, dimmer, FE_root,
main3dhud) use 6 action opcodes our APT VM (`crates/skate-game/src/apt_vm.rs`, HUD-only until now) rejected with
"Unsupported APT opcode": 0x26, 0x30, 0x42, 0x43, 0x4A, 0xA5.

### What retail does
Retail dispatches actions through a table of 256 handler pointers at `0x82FC9BF0` (TU3 image; entry 0x8E =
DefineFunction2 `82E73010`, 0x9B = DefineFunction `82E72EC0`, which confirms the base). Handlers:
- **0x26 trace** `82E6D958`: pops one value, converts it to a string and prints `AptTrace: %s` (`0x82063488`) to the
  debug output.
- **0x30 random** `82E6DDD0`: n = ToInteger(top); result = rand() % n (unsigned `divwu`; `twllei` traps on n = 0);
  pops and pushes the integer. rand is `82E82528`, a Mersenne-Twister-style generator (tempering, refill `82E823A8`).
- **0x42 initArray** `82E6E9C0`: n = ToInteger(pop); creates an array; element i = the i-th value from the top;
  length n; pops n, pushes the array (n <= 0: empty).
- **0x43 initObject** `82E6EB00`: n = ToInteger(pop); creates an object; for i = 0..n the pair is value = top, name =
  the value below it; pairs are set from the top pair down (so for a repeated name the pair deepest in the stack,
  i.e. written first in the source, wins); pops 2n, pushes the object.
- **0x4A toNumber** `82E702F0`: numbers stay; values the NaN test `82E67400` rejects become NaN; otherwise a string
  without '.' (search for `"."` at `0x820634A4`) becomes an integer via ToInteger, one with '.' a float.
- **0xA5** `82E73F50`: pushes its string operand, then tail-calls the getMember handler `82E705C0` (0x4E): pop name,
  pop object, push object[name] (EA's string-operand GetMember, like 0xA4 / 0xAF for variables / constants).
- **ToInteger** `82E5F2A8` (used by random, initArray, initObject, toNumber): int as is; float clamped to i32 and
  truncated; bool 0 / 1; string: "0x..." (length > 2) via strtol base 16 `82F53468`, else atoi `82F4DEC0`; values
  without the valid flag give 0; other types compare against the global value `[0x8307F3A8]` (0 if equal, else 1).
These match standard SWF / AVM1 Trace, RandomNumber, InitArray, InitObject, ToNumber and GetMember; retail's
handlers decide the details above (integer results, the '.' rule, atoi-style string parsing).

### Change
- `apt_vm.rs`: the six opcodes; `pub fn to_integer` (82E5F2A8) and `pub fn to_number` (82E702F0); `Host::trace`
  (default no-op, so the HUD host is unchanged) receives trace output; `Vm::seed_random` seeds the script random
  generator.
- Safety on untrusted (mod) movies: initializer count capped at 256 and checked against the stack before anything is
  popped; random with n = 0 gives 0 where retail traps; 0xA5 without a string operand is an error, not a panic;
  all errors go through the existing `Result` path and execution budget.
- Determinism: random uses a seeded xorshift64* owned by the VM (fixed default seed), so replays and a future
  multiplayer session get the same values from the same seed. Only the `rand % n` contract is retail's; retail's
  own generator state is runtime data and not reproduced.

### Verification
- `cargo test -p skate-game --bins -- apt hud`: 19 passed, 4 ignored (data-gated), including per-opcode unit tests
  (`apt_vm::tests`: trace pop + host output, random range / determinism / n = 0 / underflow, toNumber and ToInteger
  rules, initArray order / underflow / limit / negative count, initObject pairs and duplicates, 0xA5 member read)
  and the existing HUD tests unchanged.
- `apt_vm::tests::retail_menu_movies_use_no_unknown_opcode` (skipped when no local data): runs every action stream
  of the 16 decoded movies (183 streams) with a stub host and executes every opcode they contain in isolation; no
  "Unsupported APT opcode". Data: `python tools/claude-skills/recomp-research/tools/apt_actions_json.py
  <fedata.big> .local/research/fe-menus/actions <movie paths>` (local only, never committed); the test reads
  `SKATE3_FE_ACTIONS` or `.local/research/fe-menus/actions`.

### Open
- The exact string grammar of the retail NaN test `82E67400` (leading spaces, signs, exponents) is only partly read;
  ours accepts what Rust's float parser accepts plus "0x" hex.
- Frame labels: the decoded movies list 151 labels; reaching them needs the display side (imports, screen manager),
  milestone 3 later parts.

## Milestone 3b: cross-movie imports (2026-10-08)

### Problem
The menu movies are built from shared library movies: core_menu imports `Button Item` (button_item2),
`HighlightText` (menu_part_text_hilite), the picker (menu_picker) and `Image_Renderer_Component` (image_renderer);
options imports panel and highlight_option / selector / slider; the libraries import further libraries
(menu_picker -> menu_part_text_hilite -> menu_part_glow, panel -> button_menu -> button_item2). An imported character
id has no local definition, so our player (`apt_movie.rs`, HUD-only, whose movie is self-contained) fails with
"APT unknown character" as soon as a menu movie places one.

### What retail does
[data] The root character of a movie (type 9) has an import table (count / offset at body +32 / +36, 16-byte records:
source path, symbol name, local character id, runtime slot) and an export table (+40 / +44, 8-byte records: name,
character id). Source paths are relative to `data/fe/` (`source/controls/button_item2`).

[code, TU3]
- **When the source loads:** the APT load queue `sub_82E7E9F8` moves an entry from requested to loading by calling
  the host load callback stored in AptInitParms +36 (parms block `0x830CE9C8`, filled by `sub_825D3840`): that is
  `sub_82CA4138`, which prefixes `data\fe\` (`0x820AED74`) and opens the movie with `sub_82CA3428` (`/` turned into
  `\`, `big:%s`, `.apt` and `.const`). When a movie has loaded, `sub_82E7E8C0` walks its import list and asks
  `sub_82E7E6E0` for each source movie by name; the movie is linked only when every source is there, otherwise it
  stays waiting. Sources are therefore loaded on demand, before the importer is linked, and are themselves already
  linked (chained imports resolved) at that point.
- **Linking** `sub_82E76070` (argument: the movie body): for each import record in table order, scan the source
  movie's export list from the start and take the first export whose name equals the record's name byte for byte
  (case-sensitive compare loop at `82E760CC`); write the source movie's character-table entry for that export's id
  into this movie's character table at the record's character id, set the character's owner reference to the source
  movie (`sub_82CA44A0`, record +12) and count a use (16-bit, saturating). No export with that name: the slot is set to
  null. The second loop of the same function only turns local sprite / button child indices into pointers.
- **Use:** placements look the character up in the movie's character table (`sub_82E810B8`), so an imported id
  behaves like a local one from then on.

### Change
- New `crates/skate-game/src/apt_imports.rs`, engine-independent: `Import`, `Export`, `MovieDecl` (from the decoded
  JSON or the pipeline's movie JSON), `library_key` (stable key: `/` separators, no `data/fe/`, no `.apt`, like
  retail's prefix and extension handling), `MovieSource` (where movies come from: the user's assets, test data),
  `resolve(root, movie, source)`: loads each source movie once, on demand, and binds every import of the root and
  of every library it reaches with retail's rule (first export of exactly that name; a chained import is followed to
  the defining movie, which is what retail's pre-linked source table gives). Output: sorted `Binding`s
  (movie, character id -> defining movie, character id) and `Unresolved` entries with a `Failure`; serialisable and
  deterministic (BTreeMaps, sorted output).
- `Movie::link_imports(key, resolution, libraries)` in `apt_movie.rs`: gives each bound id a copy of the defining
  library's character plus everything it places, its action streams and its fonts, under free ids / action keys
  from a deterministic allocator; unresolved ids stay absent (retail's null slot). The HUD path does not call it,
  so the HUD is unchanged.
- **Moddable:** `OverrideTable` holds library movies supplied by mods, keyed by library key, per owner (newest owner
  wins); `table.over(&mut base)` is a source that consults the overrides before the user's assets, so a mod can
  replace a retail library (`source/controls/button_item2`) or add a new one that its own movies import;
  `clear_owner(id)` reverts it when the mod is disabled.
- **Safety on mod movies:** missing source movie, missing export, export of an undefined id, import cycles, chains
  longer than 16 and more than 256 movies are reported as failures, never panics or loops; the copy step has its
  own depth (64) and size (65 536 characters) limits and returns errors.
- **NOT RETAIL (layout):** retail shares the source character by pointer and keeps the source movie for its
  constants; ours copies, which behaves the same because character data is immutable and our action streams are
  already decoded (no constant-pool lookups at run time).
- **NOT RETAIL YET (not decoded):** what retail does when a source movie file does not exist (the queue entry never
  reaches the loaded state; whether it waits forever or gives up was not traced) and with import cycles (each movie
  would wait for the other). Ours reports both and links nothing for them; no retail movie has either. The limits
  above are ours (safety).
- Tool: `tools/claude-skills/recomp-research/tools/apt_actions_json.py` now also writes `imports`, `exports` and the
  local character ids; the 4 libraries reached only through imports (image_renderer, menu_part_arrow,
  menu_part_glow, button_menu) were decoded as well (20 movies, local only, never committed).

### Verification
- `cargo test -p skate-game --bins -- apt hud`: unit tests `apt_imports::tests` (chain resolve, `.apt` / `data/fe/`
  key forms, missing movie / export / undefined export, cycle and chain limit, determinism, mod override and revert)
  and `apt_movie::import_tests` (library subtree copied, child ids and action streams remapped, the imported clip
  instantiates, errors instead of panics); the existing HUD and VM tests unchanged.
- `apt_imports::tests::retail_menu_movies_resolve_every_import` (data-gated, `SKATE3_FE_ACTIONS` or
  `.local/research/fe-menus/actions`): loads the 20 decoded movies and resolves every import of every movie:
  "20 movies, 19 of 19 imports resolved", none unresolved. Run 2026-10-08: 25 passed, 4 ignored.

### Open
- Lua entry point: the `OverrideTable` is the mod-facing layer, but no `sdk` command feeds it yet (needs the menu
  movies in the user's assets first, part of the screen-manager step).
- Running the linked menu movies end to end needs the pipeline to export menu movies in the player's JSON format
  (characters, fonts, language) like the HUD prepare step; then `link_imports` is called after `Movie::load`.
- The missing-file and cycle behaviour of retail (above).

## Milestone 3c: menu movies exported at setup, loaded and linked (2026-10-08)

### Problem
The APT player and the import linker (3a, 3b) only ran on research decodes in `.local/`. The engine had no menu
movies from the user's disc, and mods had no way to reach the import override layer.

### Change
- **Setup export** `tools/prepare_menu_movies.py`, run inside the HUD stage (`prepare_runtime_huds.py`, same disc
  archives and fonts as the scoring HUD; `versions.py` fingerprints it in the `hud` group, so a re-run refreshes the
  set). Starts from 12 menu screens (`MENU_ROOTS`: FE_root, core_menu, dimmer, freeskate_options, options, main3dhud,
  screenmanager, screentransitionmanager, tabs, scrollbar, sk8popup, small_popup) and follows every import, as
  retail's load queue requests each import source by name before linking (sub_82E7E8C0, milestone 3b). Writes
  `assets/private/menu-movies/` (never committed): `manifest.json` (roots, movies, hashes), `shared.json` (language
  table, fonts, native font layout, once for all movies) and `movies/<key>.json` per movie in the HUD player format
  plus `imports` / `exports`; keys are the retail import paths (`apt_imports::library_key`). Install verifies every
  hash into a staging folder and swaps it in; a failed export keeps the previous set and writes
  `menu-movies-availability.json` like the HUD. On the user's disc: 23 movies (12 roots + 11 libraries), none
  missing, every font resolved, about 9 MB.
- **Engine** `crates/skate-game/src/retail_menu_movies.rs`: resource `RetailMenuMovies`, loaded at startup next to the
  menu tables (`retail_menus.rs` plugin). Each movie: `MovieDecl::from_json` + `apt_imports::resolve` (import
  tables resolve even for movies the player cannot load yet), `Movie::load`, `Movie::link_imports`. Log
  `RETAIL_MENUS movies=<linked> imports=<resolved>/<total>`, one warning per movie that failed with the reason.
  Nothing is drawn.
- **Moddable** `sdk.menus.movie(name, path)` (command `menu_movie`): a mod supplies or replaces a menu or library
  movie by its import name with a player-format JSON file from its own folder (path contained in the package,
  16 MB cap). It goes through `OverrideTable` (newest owner wins) and every movie is relinked, so menus importing a
  replaced library pick up the mod's version (a missing export leaves the slot empty, as retail's linker does).
  Removed on mod stop / disable (`retail_menus::clear_owner` / `clear_mods`). `sdk.menus.info().movies` lists linked
  movies, import counts, failures and each mod's movies. Multiplayer: keys are stable strings, state is per-owner and
  rebuilt deterministically (sorted keys).
- **Data-gated tests from a worktree**: `apt_imports::main_checkout()` follows a worktree's `.git` file back to the
  main checkout (override `SKATE3_MAIN_CHECKOUT`), so the 3a / 3b tests now find `.local/research/fe-menus/actions`
  from `.local/wt-menus` too. The Python test finds the disc the same way through `git rev-parse --git-common-dir`
  (overrides `SKATE3_DISC`, `SKATE3_COLLECTIONS`).
- NOT RETAIL (layout): the shared language / font file and the JSON format are ours; retail loads `.apt` / `.const`
  through `data\fe\` at run time. Same content.

### Verification
- `python -m unittest test_prepare_menu_movies` (in `tools/`): key rule matches the engine, hash-verified install
  replaces the old set and keeps it on failure, contained paths; data-gated export from the owned disc into a
  temporary folder: every root exported, none missing, no unresolved font, every import's export present in its
  source movie. 4 passed (13 s).
- `cargo test -p skate-game --bin skate3rust -- apt menu` with `SKATE3_MENU_MOVIES` at a scratch export: 42 passed,
  3 ignored. `retail_menu_movies::tests`: link + mod replace + revert, missing set; data-gated
  `retail_menu_movie_set_loads_links_and_runs`: `RETAIL_MENUS movies=13 of 23 imports=25/25`, no unknown opcode in
  any stream of the 23 movies. `cargo test -p skate-mods --lib -- menu`: 4 passed (Lua `menu_movie` command).

### Open (player gaps found by loading the real set)
- 5 movies use placement clip actions (place flag 0x80, `onClipEvent` handlers): `apt_display.rs` refuses them
  (button_item2, button_menu, image_renderer, tabs, core_menu). 4 more fail only because they import button_menu
  (panel, freeskate_options, options, sk8popup). Done in Milestone 3d.
- small_popup: the Futura Shadow foreground-font rule in `apt_text.rs` is HUD-specific (looks for a `futuraheavy`
  font in the same movie); retail's pairing rule for other movies is not decoded.
- The root list is ours (the menu screens the engine will use); more screens can be added without code changes in
  the engine.

## Milestone 3d: placement clip actions (2026-10-08)

### Problem
9 of the 23 exported movies did not load: 5 place clips with clip actions (place flag 0x80, `onClipEvent`
handlers), which `apt_display.rs` refused, and 4 import one of them (button_menu).

### What retail does [code, TU3]
- **Record.** Placement +60 (sub_82E7B340 reads it at record+4+56 when flag 0x80 is set; on a new placement and on
  a move) points at `{u32 count, u32 events}`; each event is 12 bytes `{u32 flags, u32 key_code, u32 actions}`
  (sub_82E5B158 walks it with stride 12 and queues record+8). sub_82E7B090 stores the table pointer at display
  object +24 only when one is given, so a move without 0x80 keeps the old table.
- **Flags** are the SWF clip-event values: Load 0x1, EnterFrame 0x2, Unload 0x4, MouseMove 0x8, MouseDown 0x10,
  MouseUp 0x20, KeyDown 0x40, KeyUp 0x80, Data 0x100, Initialize 0x200, Press 0x400, Release 0x800,
  ReleaseOutside 0x1000, RollOver 0x2000, RollOut 0x4000, DragOver 0x8000, DragOut 0x10000, KeyPress 0x20000,
  Construct 0x40000. Evidence: the dispatch sites below, and the dynamic-handler table 0x82FC9A3C (6 pairs of mask
  and name id) mapping 2 / 0x40 / 0x80 / 0x100 to onEnterFrame / onKeyDown / onKeyUp / onData (name table
  0x83044C60). The ids for 1 and 4 sit 10 apart like onLoad / onUnload (0xCF / 0xD9) but one entry off from the
  others, so that pair is medium confidence; the dispatch sites confirm Load = 1 and Unload = 4 by use.
- **Placement** sub_82E7AF18 (sprites only): ORs every event's flags << 8 into display flags +20 for the masks
  0x201C7 (load, enterFrame, unload, key down / up, data, keyPress), registers the clip as a key listener
  (sub_82E88050) when it has keyDown / keyUp / keyPress, then dispatches **Initialize (0x200) and Construct (0x40000)
  at once** (sub_82E5B158: an action object of priority 60 / 70 run synchronously through sub_82E6A208).
- **Tick** sub_82E5B640 (every clip, playing or not): frame advance, frame controls (sub_82E810B8: init actions,
  placements), frame do_actions queued at the back (sub_82E81438 -> sub_82E78168); then **EnterFrame** (2), except
  on the clip's first tick, pushed at the **front** of the action queue (sub_82E78240); on the first tick
  (flag 0x80 in +20) **Load** (1) at the back instead, then that flag is cleared; then the children
  (sub_82E7C658). So children's enterFrame handlers run before their parent's, and all of them before that tick's
  frame actions.
- **Unload** (4): removal sub_82E55DA8 queues it at the back before tearing down the children; it runs on the
  removed clip.
- **Scope**: the queue entry's target is the clip that owns the placement (sub_82E78168 stores r5 = the clip), so
  `this` is the placed clip, as in Flash.
- **Input events** (press / release / rollOver / key*) come from the host's input through the same dispatcher;
  no movie in the set uses them (all 9 tables hold only Construct). Out of scope here.
- **0xA6** (handler 82E74028): push string operand, then setVariable (0x1D, 82E6D248). Every construct handler in
  the set is `_type = "CustomButton"` / `mButtonImage = "A"` style component parameters written with it.

### Change
- `tools/prepare_menu_movies.py`: decodes the table into `clip_actions` on the placement (bounds-checked, at most
  64 events) and exports the handler streams; menu set version 2 (the engine refuses version 1, setup re-exports).
- `apt_display.rs`: `ClipAction`, `clip_event` flag constants, `Placement::clip_actions` (kept on a move without
  0x80, as retail); a 0x80 placement without a decoded table (old export) and more than 64 events are errors.
- `apt_movie.rs`: `Pending { object, offset, event }` queue; `create` queues Initialize then Construct in
  `immediate` (run before anything queued) before the clip's first frame, Load after its first frame's actions;
  `advance` pushes EnterFrame of every clip that existed before the tick to the queue front, in creation order (so
  children first); `remove` queues Unload; `next_action()` drains immediate first and skips handlers of removed
  clips except Unload. `link_imports` copies and renumbers clip-action streams. `hud_runtime.rs` drains through
  `next_action()` (HUD movies have no clip actions: unchanged).
- `apt_vm.rs`: opcode 0xA6.
- Mod movies use the same path (same JSON, same limits).
- NOT RETAIL (timing): retail runs Initialize / Construct inside the placing action; ours right after the running
  action, still before every other queued action. NOT RETAIL YET (order): retail queues a parent's frame actions
  before its new children tick (Load); our seek places children first (existing frame-action order).

### Verification
- `cargo test -p skate-game --bin skate3rust -- apt hud menu` with `SKATE3_MENU_MOVIES` at a scratch export:
  47 passed, 4 ignored (unrelated data-gated tests matched by the filter). New `apt_movie::clip_action_tests`:
  dispatch order (initialize, construct, the clip's frame action, load; next tick enterFrame; unload runs on the
  removed clip; nothing after removal), scope (construct sets `_type` on the placed clip, not the parent, through
  0xA6), unsafe tables (old export without a decoded table, more than 64 events) are errors and a move keeps the
  table. Data-gated `retail_menu_movie_set_loads_links_and_runs`: **`RETAIL_MENUS movies=22 of 23
  imports=25/25`** (was 13), no unknown opcode in any stream including the clip-action streams.
- HUD: `scoring_runtime::tests::landed_multiplier_reaches_hud_across_repeated_tricks` (with `SKATE3_ASSET_ROOT`)
  passes; the HUD drain only changed to `next_action()`.
- `python -m unittest test_prepare_menu_movies`: 5 passed (new: table layout and bounds); the real export decodes
  9 tables, all Construct only.

### Open
- small_popup (the last of the 23): the Futura Shadow foreground-font rule (done in Milestone 3e).
- Input-driven clip events (press, release, rollOver, key*) wait for the menu input milestone; the dispatcher
  shape (`queue_events`, flag constants) is ready for them.
- The onLoad / onUnload name ids in table 0x82FC9A3C are one entry off from the others (see above); behaviour
  does not depend on it.

## Milestone 3e: the Futura Shadow secondary font in every movie (2026-10-08)

### Problem
small_popup, the last of the 23 movies, failed with "Missing native Futura Shadow foreground font". Our rule
(apt_text.rs, written for the HUD) looked for the foreground font among the movie's own APT font characters;
small_popup places "Futura Shadow" but no character that uses futuraheavy.

### What retail does [code, TU3]
- Native font table: FontManager sub_82808AE8 builds up to 12 rows of 36 bytes at 0x830680D0 from the font records
  (class 0xFECFBCAF356518C4, the list picked by language mode): row +0 file name, +4 APT name, scale / offset
  floats, +32 the font (loaded lazily by sub_82809308 from `data\fe/fonts/<file name>.bmpFont`, sub_82807870).
- Lookup sub_82809208(name): case-insensitive compare (sub_82AE8A10) of `name` against each row's +4 name; on a
  miss, the row whose +0 name is "debug" (0x821A06E0); else row 0. It never fails.
- Text font setup sub_825D6B68: primary = lookup(APT font name); if that name equals "Futura Shadow" (0x8220BD44,
  sub_82AE89B0) the text also gets a secondary font = lookup("futuraheavy") (0x8220BD54) with X adjustment 1 and
  the other 0. sub_82CA1FD8 draws the primary black, then the secondary in the text colour. The secondary comes
  from the global native table: it does not depend on the movie using that font.

### Change
- `apt_text.rs`: `Font::foreground` is now the secondary native font itself (`Option<Box<Font>>`), not an APT
  character id. `RETAIL_FONT_PAIRS` holds retail's one pair; data may replace it with `font_pairs` (APT name ->
  native file name; the menu export writes retail's table into shared.json, an empty map turns pairs off). The
  partner is found in the whole native table (`font_mappings` by file name), case-insensitive. A missing partner
  (old export, a mod's data) leaves the text with its primary pass: no error, no panic.
- `apt_movie.rs` (import linking) copies the secondary with the font; `apt_scene.rs` draws it directly;
  `scoring_hud.rs` loads its texture too. HUD output unchanged (same fonts, same passes).
- `tools/prepare_menu_movies.py`: `FONT_PAIRS`; the export adds each pair's partner font to shared.json even when
  no movie places it, and writes `font_pairs`. Older installs still load (retail table default; without the
  partner asset the shadow text draws its primary pass only): re-running setup group `hud` adds it.
- Moddability: a mod's movie (same JSON path) gets the same rule; a mod font missing its partner falls back
  without error.
- NOT RETAIL (fallback): retail's lookup falls back to the "debug" row; we have no debug bank and draw the
  primary pass only.
- OPEN QUESTION (match field): the static data says row +4 is the APT name (the debug row's is "NOT FOR USE IN
  APT", the futuraheavy row's is "Futura Std Medium"), so lookup("futuraheavy") would miss and fall back to the
  debug font. We keep the HUD's established match by file name (futuraheavy), which the HUD port matched
  visually. A recomp trace of sub_82809208's return for "futuraheavy" would settle it.

### Verification
- `cargo test -p skate-game --bin skate3rust -- apt hud menu font` with `SKATE3_MENU_MOVIES` at a scratch export:
  51 passed, 4 ignored. New `apt_text::font_pair_tests` (4): Futura Shadow gets futuraheavy from the native table
  without a character using it; missing partner asset or table row gives the primary pass only; `font_pairs` data
  replaces the retail table (case-insensitive, empty map turns pairs off, unknown file falls back); other fonts get
  no partner. Data-gated `retail_menu_movie_set_loads_links_and_runs`: **`RETAIL_MENUS movies=23 of 23
  imports=25/25`**; the test now fails on any load failure.
- HUD unchanged: `scoring_runtime::tests::landed_multiplier_reaches_hud_across_repeated_tricks` (with
  `SKATE3_ASSET_ROOT`), `scoring_hud::tests` and the other scoring tests pass.
- `python -m unittest test_prepare_menu_movies`: 5 passed; the scratch export holds 23 movies, no missing movie,
  no unresolved font, `Futura Std Medium` in shared.json.

## Milestone 3f: placement clip depth (2026-10-08)

### Problem
A SWF placement can carry a clip depth: the placed shape is then a mask for the depths above it up to the clip
depth. Our display list parsed the field (`apt_display.rs`) but the drawing path (`apt_scene.rs`) ignored it, and it
was unknown whether retail APT draws such masks (stencil, scissor) and whether the menu movies use them.

### What retail does [code, TU3]
- **Read.** `sub_82E7B340` (placement record at r31 = control + 4: +0 flags, +4 depth, +8 character, +12 matrix,
  +36 colour transform, +44 ratio, +48 name, +52 clip depth, +56 clip-event table) loads +52 and passes it to
  `sub_82E7AE98` -> `sub_82E7B090` on every path that creates a display object: flag 2 (new or replaced character)
  and a move (flag 1) onto an empty depth. It does not test flag 0x40. A move of an existing object
  (`loc_82E7B5F8`) passes -1, and `sub_82E7B090` only hands the value to the create call `sub_82E7A950`, so a move
  never changes a stored clip depth.
- **Store.** `sub_82E7A950` writes it as a halfword: `sth r23,22(props)` (props = display object +32 -> +4; +20 is the
  depth halfword). Clones copy it (`sub_82E832C8`, `sub_82E83BC0`).
- **Use.** Over the whole recompiled image only two code sites read props +22: the getter `sub_82E48930` (and
  `sub_82E461A0` on props), which no code calls and no data table points at, and the bounds walk `sub_82E59C28`
  (called by `sub_82E59DA8`, the local-bounds function behind sizes and hit areas), which leaves out every child whose
  clip depth halfword is below 0x8000 (a mask). No other reader exists, the APT renderer (game side, 82CA range) has
  none either, and no load of the word at +20 extracts the low half. So **retail draws a clip-depth shape like any
  other shape and masks nothing**; the only effect is that masks do not count towards a clip's bounds. Script masks
  (`setMask`, warning string 0x82062250) are a separate mechanism, not used by these movies.
- **Data.** All 23 retail menu movies: 1434 placements, every clip depth -1, flag 0x40 never set. Both HUD movies:
  1233 placements, all -1. No retail movie we ship uses a clip depth.

### Change
- `apt_display.rs`: clip depth follows retail: taken from the record whenever a placement creates the object
  (flag 2) regardless of flag 0x40, stored as 16 bits (`as i16`), missing value = -1; a move keeps the stored
  value (flag 0x40 on a move no longer errors or changes it). Doc comment on `Placement::clip_depth`.
- Drawing (`apt_scene.rs`) unchanged on purpose: retail draws the shape and masks nothing, so no stencil or
  scissor. Bounds: our player has no sprite bounds yet (`_width` / `_height` exist only for text,
  `apt_movie.rs` text setup); when sprite bounds land they must skip children with clip depth >= 0
  (`sub_82E59C28`).
- Mods: a mod movie with clip depths draws exactly as retail would (no mask); values are clamped to 16 bits like
  retail, no panic path.

### Verification
- `cargo test -p skate-game --bin skate3rust -- apt hud menu font` with `SKATE3_MENU_MOVIES` at a scratch export:
  52 passed, 4 ignored. New `apt_movie::clip_action_tests::apt_clip_depth_follows_retail_placement` (stored without
  flag 0x40, kept on a move with flag 0x40, replaced with the character, 16-bit storage, missing = -1, removed with
  the object). The data-gated set test now counts clip depths: **`RETAIL_MENUS movies=23 of 23 imports=25/25
  placements=1434 clip_depth=0`** and fails if a retail menu movie ever carries one.
- HUD unchanged (all its clip depths are -1 and drawing does not read the field):
  `scoring_runtime::tests::landed_multiplier_reaches_hud_across_repeated_tricks` (with `SKATE3_ASSET_ROOT`) passes.

### Open
- Retail creates an object when a move (flag 1 only) targets an empty depth; our display list still refuses that
  ("APT move references empty depth"). Not hit by any retail movie.
- A recomp trace could confirm at run time that nothing reads props +22 while a menu draws (static evidence only).

## Milestone 4: bitmaps and textured fills (2026-10-08)

### Problem
The menu movies are mostly textured shapes (panels, glows, arrows, tab icons), but the player could only describe
shapes that always have a texture (the HUD case): solid units did not load, imported shapes lost their geometry
(`link_imports` gives them a new id in the importing movie) and nothing named a bitmap, so a mod could not replace
one.

### What retail does
- [data, the user's disc, 23 menu movies] 154 bitmap characters (type 7, `{texture_id}` only). None is ever placed:
  placements reference sprites (204), shapes (193) and text (25) only. Every bitmap is drawn as the texture of a
  shape unit. Shapes come pre-tessellated in each movie's `.geo` (no gradients or curves at run time): 231 units,
  207 `texture_clamped` (render type 2) and 24 `solid` (type 1); no `texture_wrapped` (3) or `line` (0). Every unit
  resolves; the 207 textured units use all 154 bitmaps (30 are used by more than one unit). The 2 HUD movies
  (trickdisplay and its fonts) have only textured units.
- [data] GEO unit layout (`tools/vendor/skate3_ui/geo.py`): +0x00 render type, +0x04 RGBA colour (floats), +0x14
  bitmap character id, +0x18 UV matrix (6 floats, applied to the vertex position, in texels), +0x30 triangle count,
  +0x34 triangles. The bitmap character's `texture_id` names the texture `<movie>_textures\<id>.Texture` in the
  movie's texture package (fetexture.big / fedata.big RX2), decoded to RGBA8 at setup (158 payloads incl. fonts).
- [code, TU3] The movie loader `sub_82CA3428` (AptInitParms +36 callback chain, doc "Milestone 3b") opens the three
  files of a movie from the `data\fe\` table at `0x820AED00` (`.apt`, `.const`, `.geo` at `0x820AECF8`) into
  {pointer, size} slots at record +532 / +540 / +548: the GEO blob is kept as authored. The texture name format
  `%s_textures\%i.Texture` is at `0x820AED9C`, with a fallback `missingtexture.Texture` at `0x820AEDB4`.
- [code, TU3] Draw chain (details in the local front-end menus note, "APT shape renderer"): `sub_82CA4020` builds
  the movie resource, `sub_82CA30B8` relocates the GEO in place, `sub_82CA2A40` makes one record per shape and
  `sub_825D5DC8` one draw object per unit: type 1 -> solid object `sub_825D5800` (draw `825D5AE0`), types 2 / 3 ->
  textured object `sub_825D4FC0` (draw `825D5580`), any other type (line 0) -> nothing. The textured draw computes
  one colour = unit colour * colour-transform multiply + add, draws nothing when its alpha <= 0, and hands 6
  vertices (2 triangles), the UV rows from unit +24..+44, the texture and that colour to `TheSimpleDraw`
  (`sub_82805368`, shaders `simpledraw_*` in shaders_final.big).
- Not decoded yet: the sampler (filter, and which of types 2 / 3 clamps); see Open.

### Change
- **Setup** (`tools/prepare_menu_movies.py`, set version 3): each textured unit also carries `bitmap`, the GEO unit
  +0x14 id it was resolved through (`_with_bitmap`). Re-export on the next setup run (the `hud` group fingerprint
  covers the tool). No new files: the RGBA payloads were already exported.
- **Scene** (`crates/skate-game/src/apt_scene.rs`): `Fill` (GEO render type), `Shape.texture` optional (solid and
  line units), `Shape.bitmap`, `Vertex.uv` optional; `Draw.fill` tells the renderer clamp or wrap addressing (text
  draws are clamped); a solid draw has texture `""`. Line units are not drawn (retail `sub_825D5DC8`). Shapes come
  from a `ShapeSource`; for menus (`retail_unit_colour`) a unit draws with retail's single colour
  `unit colour * multiply + add` and is skipped at alpha <= 0 (`825D5580`). The HUD keeps its `Shapes` table and
  its earlier `texel * multiply + add` (output unchanged; see Open).
- **Imports** (`apt_movie.rs`): `Movie::shape_origins` records, for every shape `link_imports` copies, the library
  and id that define it (following nested imports to the defining movie).
- **Menu set** (`retail_menu_movies.rs`): GEO units per movie key (validated: textured units need a bitmap,
  finite values; a bad movie is a failure, not a panic); `scene(key, movie)` gives the `ShapeSource` for a running
  instance; `image(texture)` returns the retail payload (path in the set + size) or a mod image.
- **Stable id and mod entry point**: a bitmap is `<movie key>#<bitmap character id>` in the movie that defines the
  shape. `sdk.menus.bitmap(movie, bitmap, path)` (command `menu_bitmap`) replaces it with a PNG from the mod's
  folder (4 MB file, up to 2048 x 2048, 64 per mod), decoded to RGBA8; the image covers the same fill (same UVs,
  normalised), newest mod wins, removed on mod stop / disable with the rest of the mod's menu content
  (`clear_owner` / `clear_mods`). `sdk.menus.info().movies.bitmaps` lists every id (`ids`) and each mod's
  replacements (`mods`). A whole movie can still be replaced with `sdk.menus.movie`.
- **Fixed**: `menu_movie` validated its path as a `.glb` asset, so every Lua `sdk.menus.movie(..., 'x.json')` was
  rejected at submit; it now takes a `.json` content path.
- Deterministic and local: draws follow the display list order; ids are strings sorted in `BTreeMap`s.

### Verification
- `python -m unittest test_prepare_menu_movies` (in `tools/`): new `test_textured_units_get_their_bitmap_id`; the
  data-gated export checks that every shape resolves and every textured unit names a bitmap character whose texture
  id is the payload's. 6 passed.
- `retail_menu_movies::tests::menu_bitmaps_draw_through_imports_and_mods_replace_then_revert`: a menu importing a
  library sprite draws the library's solid and textured units (colour, UVs, placement), the retail payload resolves,
  a mod bitmap replaces it by stable id (invalid ids, sizes and movies refused), newest owner wins, stop reverts.
  `menu_units_follow_the_retail_colour_rule` (line unit not drawn, alpha 0 skipped, add folded into the colour),
  `menu_shapes_reject_textured_units_without_bitmap`. skate-mods: `menu_movie` / `menu_bitmap` validation.
- Data-gated `retail_menu_movie_set_loads_links_and_runs` now also walks every shape of every linked movie
  (local and imported) and draws each movie's first frame: **`RETAIL_MENUS movies=23 of 23 imports=25/25
  placements=1434 clip_depth=0 shape_units=528 textured=481 solid=47 bitmaps_used=154 bitmap_ids=154
  frame0_draws=863 frame0_bitmap_draws=835`** (unit counts are per linked movie, so library shapes count once per
  importer; every one of the 154 bitmaps is reached and its payload has the exported size).

### Open
- Sampler: the tfetch filter / address bits of the `simpledraw_*` pixel shaders are not decoded (disassembler fails
  on constant-less shaders; notes "APT shape renderer" OPEN), nor which of types 2 / 3 clamps. Ours names the
  addressing from the render type (`Draw.fill`); the screen-manager renderer must take filtering from that decode.
- The HUD movie has 233 placements with colour-transform add terms; retail folds add into the unit colour
  (`825D5580`), ours still adds after the texel for the HUD. Switching the HUD to `retail_unit_colour` is a one-line
  change that changes its look, so it waits for an in-game check. The solid draw `825D5AE0` is assumed to use the
  same colour rule (to read).
- `missingtexture.Texture` (`0x820AEDB4`): retail's fallback for a missing payload; ours fails the movie at setup
  instead (no retail movie misses one).
- Drawing the menus on screen comes with the screen manager milestone (the renderer maps `Draw.texture` through
  `RetailMenuMovies::image`).

## Milestone 5a: the front-end stage (2026-10-10)
Branch `ui/retail-menus-2` (after #59 merged). Retail's screen manager is a script (`helper/screenmanager.apt`);
the C++ only calls it by name and answers its callbacks. So the port runs the retail movies themselves on one stage.

### Problem
The player ran one movie per VM (the HUD). The menus need FE_root, the screen manager and each screen movie in ONE
script VM sharing `_global`, screens loaded into clips at runtime (MovieClipLoader), clips created by script
(createEmptyMovieClip), component classes bound by `Object.registerClass`, and each movie at its own frame rate.

### Evidence
- [code] C++ calls the AS by path string: names at 0x8302C608 + index * 4 (OpenScreen 6, CloseScreen 8,
  DestroyScreen 9, SetScreenState 10, UpdateInput 11, ShowScreen 12, RegisterTemplate 16), via sub_825D3B38 /
  sub_8263F320. Template table 0x8302F1A8 (133 records of 16 bytes: name, swf path, type, -1), e.g. 69 CrossBar
  `main/core_menu.swf`, 90 Options, 131 TipsLoader. OpenScreen (sub_82D09140) passes ("source/screens/" + path,
  layer, "0"/"1", template name). Callbacks OnScreenLoaded / IntroComplete / OutroComplete (825A4A70 / 825A4AF8 /
  825A4B70) index their slot with id - 600 (AS id = 600 + template index, the AS hands out depths from 600).
  Research notes are kept locally.
- [code] Retail pop (opcode 0x17, handler 82E6D048) pops only above the stack base; on an empty stack it does
  nothing. screenmanager.apt relies on it (a pop after a setMember at 0x875).
- [data] Libraries declare their component classes in their ROOT init actions
  (`Object.registerClass("Menu_Picker_Component", menu_picker)` in menu_picker.apt); importers name the clip by
  that linkage name (core_menu imports character 166 as Menu_Picker_Component). Retail loads every library before
  linking (sub_82E7E8C0), so those init actions run.

### Change
- `apt_stage.rs` (new): `Stage` = one `Vm` + loaded movies (`Level`), `StageNatives` (the C++ host objects: Game,
  CoreMenu, ...), `MovieSource` (linked movies with mod overrides). MovieClipLoader (addListener, loadClip queued to
  the next update like retail's load queue, unloadClip, onLoadStart / Complete / Init), createEmptyMovieClip,
  removeMovieClip, getDepth, `Object.registerClass`, library root init actions once per stage, `call_path` for the
  C++ calls (`_global.ScreenManager.OpenScreen`), per-movie `milliseconds_per_frame` (header of character 0).
  Retail `.swf` template paths map to the packed `.apt` key.
- `apt_movie.rs`: `initialize_into` (a loaded movie's root reuses the target clip's object, so script references
  such as ScreenManager.mMovieClipList stay valid), `unload`, `milliseconds_per_frame`, `linkage`,
  `import_files`, `classes` + `CLASS_CONSTRUCTOR` (clip gets the class prototype; constructor runs after its
  initialize / construct handlers; the exact retail point is not decoded).
- `apt_vm.rs`: retail pop rule; String methods (length, indexOf, lastIndexOf, substring, substr, slice, charAt,
  charCodeAt, case) and Array methods (push, pop, shift, unshift, splice, slice, concat, reverse, join), standard
  AS2 semantics; `call_function`; `last_instruction` for error reports.

### Verification
`cargo test -p skate-game apt_` (23 passed, 1 ignored). `retail_screen_manager_opens_a_screen` (data-gated, the
exported set): FE_root traces "FE_ROOT SETUP!", screenmanager registers `_global.ScreenManager` (currentDepth 600),
RegisterTemplate("CrossBar"), OpenScreen with the retail CrossBar arguments, the next update loads core_menu
with its imported components, and the movie calls Game.OnScreenLoaded(600) then Game.OnScreenIntroComplete(600).
The only call left without a target is `screen_class.OnLoaded()` on core_menu's class, which defines none (an AS2
no-op).

### Open
- Root timeline loop: RESOLVED in Milestone 6 (below): loaded roots do not play.
- Library init order relative to the importer's own init actions, and the registerClass constructor point.
- `_root` of a loaded movie (ours: its own root; Flash: `_level0` unless locked).
- Drawing the stage (levels with their parent clip transforms) and the C++ host (CoreMenu, Game) are next.
- Input repeat timing (`.repeat` actions in input.cfg) is not statically findable; a guarded recomp hook is
  proposed in a local research note.

## Milestone 6: pause menu navigation and the CoreMenu object (2026-10-10, in progress)

### Evidence
- [code] Loaded movie roots do not play: load completion sub_82E7EF28 gives the root clip flag 0x80, not the
  playing flag 0x40 (set only by play(), action 0x06 handler 82E6BCC8, verified; stop 0x07 82E6BDB8 clears it).
  The tick 82E5B640 advances 0x80 clips only when the APT config byte [0x83037510+64] is set; Skate 3's init
  (sub_825D39F0 -> sub_82E654A0) leaves it 0. So screenmanager.apt's 2-frame root runs `new screenmanager(this)`
  once (a local research note; how frame 0 is first entered is not traced).
- [code] Crossbar handler sub_8261B560 (a local research note; verified: the eight fe sound
  keys against fe_callers.txt, the six-entry SetState registration in 8261B9C8, the row walk 8261F0E8 read in full):
  up / down move the row with wrap and walk to an enabled row (two passes, else row 0 and no sound / SetState),
  tab left / right wrap, reset the row and post RefreshMenu (0), select opens the sub column (SetState 3) for items
  with a sub-option kind or runs the item, back at level 1 closes the pause menu (front-end screen, FE, id 56), at level 2 returns to
  level 1. Sounds crossbar_up / down / left / right, core_a / b / x_button.
- [code] CoreMenu: 21 native members (registrar sub_825B68E0; a local research note), all
  read-only views of the crossbar screen and the mode menu except SetOnlineTeamSubOptionDone.

### Change
- `apt_stage.rs`: loaded roots start stopped (above); `retail_screen_manager_closes_a_screen` now passes
  (OnScreenOutroComplete(600) after CloseScreen("600")).
- `skate-core/src/crossbar.rs` (new): `CrossbarState` (the screen fields, serialisable), `CrossbarView`,
  `NavEvent` (retail key indices), `CrossbarRules` (sounds and SetState numbers as data, retail defaults),
  `handle` (port of sub_8261B560 / 8261F0E8 / 8261D9F8 / 8261BC30 / 8261DA58), `RegistryView` (rows and enabled
  rules from the menu registry, so mods change the rows).
- `skate-game/src/crossbar_host.rs` (new): the 21 CoreMenu members over the registry and the crossbar state.

### Drawing and FELanguage (same milestone)
- `Stage::draw` gives each loaded movie's draws under the clip it was loaded into (ancestor matrices, visibility,
  alpha); `apt_scene::draw_from`. Font atlases join the menu image map (`RetailMenuMovies::image`). The open pause
  menu gives 24 draws (22 textured), all textures resolved (data-gated test).
- Text fields get `_height` (field height; core_menu centres its DLC label with it). NOT RETAIL YET: retail's
  autosize height rule is not decoded.
- `fe_language.rs` (new): FELanguage GetLocalizedFormatNumberString / FormatString / FormatStringCustom
  (registrar 825AA5E8; a local research note; verified the example strings
  `ID_CROSSBAR_BOARD_SALES_MILESTONE` "Board Sales Milestone {0:d}/{1:d}" and `ID_COMMON_PERCENT` "{0:d}%" in the
  exported language table). Placeholders `{N:type[.P]}`, `{{` literal, separators as data (`NumberFormat`,
  retail `. , 3`). NOT RETAIL YET: int vs float is taken from integrality (our VM has only f64); typed formats
  other than d / f are not decoded.
- `crossbar_host::FrontEnd`: the stage's host objects (CoreMenu, FELanguage; Game / Audio calls recorded).
- [data, verified] Sub-option label table 0x83027958 and helper table at +168 (7 kinds x 6 pointers; kind 0
  labels NEW_PARK .. TUTORIAL against helpers MY_PARKS .. PARK_EDITOR, retail pairs them by index); a local research
  note (extraction next).

### The retail boot and the first picture (same milestone)
- [code + data] The front end's `_level0` is `screens/main.apt` (sub_825D7E10 loads `source\screens\main` into
  `_level0`; a local research note). Its root defines Register*Enum, loads main3dhud /
  autosave / fmv_popup into `_level1..3` with getURL (opcode 0x83, handler 82E72590: "FSCommand:" or a ".swf"
  load) and fills layer clips with loadMovie: gScreenManager (screenmanager.swf), gChyron, gSubtitle,
  gFadeBackground. FE_root.apt is template 12 "Background" (verified), a screen, not the root.
- [code, verified] Crossbar IntroComplete (vtable 0x82303F90 +104 = 8261B1F0) -> mode selector 8261B9E8 ->
  SetState(InitialiseMenu) through the facade; the AS compares against FECrossBarScreenMode_* registered by C++
  through `_level0._root.RegisterChallengeEnum(name, "%d")` (path entry 34), which stores `_global[name]`.
- [code] Named DefineFunction2 stores into the current scope (82E73010 -> 82E68770): locals in a function, the
  timeline in a frame action (ours used `_global`).
- [data] Name lookup ignores case when no exact match: the only spelling in the image is "CoreMenu"
  (0x82300C68) while core_menu.apt reads `coremenu`.
- Export set version 4 adds `source/screens/main` and getURL operands (`tools/prepare_menu_movies.py`,
  `tools/vendor/skate3_ui/actions.py`); the user's install needs the hud setup group re-run (automatic).
- Stage: getURL `_levelN` loads, MovieClip.loadMovie, text changes re-localise (`property_changed`).
- Dev view (`retail_menu_stage.rs`, `SKATE3_RETAIL_MENUS=1`, F8; `SKATE3_MENU_MOVIES` points at a dev export):
  boots main.apt, registers the crossbar enums and the CrossBar template, opens core_menu with the retail
  arguments and posts InitialiseMenu on IntroComplete. In game (a local screenshot): the retail
  Free Play pause menu, Port Carverton banner, tab icons, and the Free Play Main rows Challenge Map, Resume
  Career, Free Play, Call Skater, skate.Profile: Career (greyed: no handlers yet, the agreed rule).
- Missing frame labels leave the clip where it is (Flash behaviour; retail not traced).
- main.apt needs three more opcodes: 0x3F modulo (82E6E5F8: ToNumber both, C fmod, f32 result), 0x4B ToString
  (82E70458), 0x44 typeof (82E6ECC0; standard AS2 names, NOT RETAIL YET: retail's name table not read).
- Data-gated tests skip an installed set older than the engine reads (`apt_imports::test_menu_set`); pass
  `SKATE3_MENU_MOVIES=<export>/runtime` to run them against a fresh export.

### Open
- FIXED (title / category overlap): a clip whose `_x` / `_y` a script set no longer follows its timeline's
  placement (`Movie::script_moved`, stage only; Flash rule, NOT RETAIL YET: retail's flag is not traced). The
  header now reads "Free Play > Main" like retail.
- Selected-row highlight works (see "Row handlers"). The blur behind the pause menu: see "Pause blur" below.
- Boot registration ported (`retail_menu_stage::boot_registration`, on Game.ScreenManagerReady like retail
  825D9450; a local research note, verified path indices 28 / 30 / 31 / 32 / 33 / 46): the
  56 Apt keys as RegisterAptInputEnum(name, key INDEX) (retail registers the index, not the record's key code),
  FE_SOUND_<NAME> = record name for all fe records (from the install's audio manifest), SetPlatform "0",
  SetScreenSize 1280 / 720, UpdatePALMode false (SetScreenEdgeOffset 136 only on SD, so not sent since 2026-10-24). Audio.PlaySound from the movies plays
  the fe record. Open: the 191 challenge enums, RefreshSubtitle.
- Screen templates extracted (`skate-data` menu_tables `templates`, located by the record whose path is
  "main/core_menu.swf", walked both ways): TU3 133 records at 0x8302F1A8 (0 Ignore, 12 Background = FE_root,
  69 CrossBar, 90 Options, 131 TipsLoader; test `tu3_image_matches_the_documented_tables`); the disc build's table
  is at 0x82FD33E8. The stage registers all of them in order, so the pause menu's AS id is 669 like retail (600
  with tables extracted before this; setup re-extracts because `asset_pipeline/menu_tables.py` now requires
  templates).
- Sub-option tables: done (see "Sub-options").
- Label overrides (SignIn / sign out, ReplayEditor instant replay, PartyPlay literal) and the online team pending
  path are not ported. GetProgressionData reads context facts (`progression_*`), -1 (panel hidden) by default.
- Hooking CoreMenu and the handler into the stage, the Game natives, drawing, input and the FE screen stack are next.

## Pause blur: the world behind FE screens (2026-10-10)

### Problem
Retail blurs and darkens the world behind the pause menu; our dev view drew the menu over a sharp world, and how
retail switches the blur on for the pause menu was not known (`pause-backdrop.md`, `pause-blur.md`).

### Evidence (TU3 recomp addresses; research note on the pause blur link, all verified)
- [code] Pause open (sub_825EB998) queues a push of FE id 56 on the FE screen stack. The stack's push / replace /
  pop handlers (sub_82D09B00 .. sub_82D09DE0) call vt+28 with byte +4 of the top screen's record in the info table
  at [stack+148]; pop applies the new top's byte, an empty stack applies 0.
- [code] vt+28 of the stack vtable 0x823048E0 (pointer at 0x823048FC) = sub_825EBED8: stores the flag at FE manager
  +461 and, on an off -> on change only, resets the tint (+476 / +480 / +484) to 255 and the strength (+468 / +472)
  to 0.0.
- [data] The info table: 0x83026088, 72 records of 24 bytes (stack init sub_825E8C40: `li r8,72; addi r7,r9,24712`,
  `stw r8,132(r25)`). Byte +4 is set for 32 screens, including 56 (the pause crossbar) and 69. The 64-bit value at
  +16 is the same per screen in every build (the disc build has 70 records, TU3 inserts two at ids 31 / 32; equal
  to TU3's key by key, test `disc_executable_gives_the_same_tables`), but four keys are shared (TU3 ids 0 / 32 / 49,
  5 / 6 / 7 / 8 / 22, 9 / 10, 12 / 13; a local research note, verified in both builds).
  Our screen id is the key, plus `:n` (position among the records sharing it) when shared; a shared-key id can
  differ between builds (TU3 49 = `:2`, disc 47 = `:1`), setup always extracts the disc's.
- [code + data] FE manager tick sub_825D8E68: while on, tint -15 per tick down to (90, 85, 81) (a global flag at
  [0x83083C38]+32764 picks (30, 30, 30) instead) and strength +0.8 (0x8208824C) up to 8.0 (0x82099250); while off,
  tint +15 back to 255 and strength -0.8 to 0. A second byte (+462) gives a blur-only ramp while off (not used by
  the pause path, not ported).

### Change
- `skate-core/src/fe_blur.rs` (new): `FeBlur` (set_enabled = sub_825EBED8, tick = sub_825D8E68 with retail's
  clamps, `advance` at the console's 30 Hz cadence so any frame rate gives the same ramp), `FeBlurParams::RETAIL`.
- `skate-data` menu_tables: `screens` (`FeScreen { id, key, blur }`), located by the stack init's code pattern (works on
  both builds); setup re-extracts the tables because `menu_tables.py` now requires `screens`.
- `skate-game/src/fe_backdrop.rs` + `fe_backdrop.wgsl` (new): the FE stack's top screen key decides on / off
  (retail flag, then mod overrides, newest last); a two-pass blur + tint on the gameplay camera after the retail
  tone pass and before the UI pass, so the menu stays sharp and the exposure meter sees the undimmed world. The dev
  view pushes the pause screen (id `b4b7976b40b3e837`, TU3 id 56) while it is open.
- Mods: `sdk.menus.backdrop(screen_id, {blur, strength, step, tint={r,g,b}, tint_step})`, nil options drops the
  mod's override, removed when the mod stops; `sdk.menus.info().screens` lists every id with its retail flag.

### NOT RETAIL YET
- The blur kernel: retail's blur pass (sub_827F7270, renderer +3292 / pass bit 0x40) is not ported; ours is a
  17-tap gaussian whose radius is the strength in pixels at 720 lines (assumed). The tint multiplies the displayed
  colour (tint alpha is 0.0 in retail; assumed multiply).
- The tick rate of the FE manager is not measured (assumed 30 Hz: about 0.37 s to full tint).
- The alternative (30, 30, 30) floor and the +462 blur-only mode are not ported.

### Verification
- `cargo test -p skate-core fe_blur`: the ramp step by step against the clamps above, restart only on off -> on,
  the same result at 30 and 120 fps.
- `cargo test -p skate-data --test menu_tables -- --ignored`: the TU3 table address, the 32 blur ids, the pause key
  at id 56; the disc table equals TU3's minus ids 31 / 32.
- `cargo test -p skate-mods`, `cargo test -p skate-game fe_backdrop`: command validation, retail flag then the
  newest mod wins, cleanup per mod.

## Row handlers: retail pause rows open our features (2026-10-10)

### Problem
Every retail pause row was greyed: the registry enables a row only when someone handles it, and nothing did.

### Change
- `retail_menus.rs`: `OVERLAY_ITEMS` registers engine handlers (owner `overlay:<SECTION>`) for ChallengeMap -> MAPS,
  SkateProfileOffline -> SKATER, GameSettings -> GRAPHICS, Extras -> EXTRAS. Retail opens its own FE screens
  there (ChallengeMap 17, GameSettings 45, Extras 18); until those are ported, the overlay menu section with the
  same job opens. NOT RETAIL YET by design (a bridge).
- `retail_menu_stage.rs`: a crossbar select goes through `RetailMenus::act`; an `overlay:` outcome closes the
  retail pause screen and opens that section (`graphics_menu::Menu::open_section`); a mod's handler gets its
  `menu_select` event as before. Mods enable or take over any row with `sdk.menus.handle`.

### Verification
- `cargo test -p skate-game retail_menus`: ChallengeMap is enabled and forwards to `overlay:MAPS`.
- A local screenshot: Challenge Map enabled and selected with retail's highlight
  (large icon, help line "Access challenges and locations"); the highlight needed no extra port.

## Screen stack (2026-10-10)

### Problem
The dev view opened and closed the pause movie directly; retail runs every FE screen through a two-stack screen
manager whose per-screen records also suspend gameplay, show the chyron and set the HUD.

### Evidence (TU3 recomp; research note on the FE stack port)
- [code, verified] Push sub_82D09C58: closes the old top (sub_82D09848) or hides it (screen vt+28) by the
  request's keep-open byte, appends, screen vt+48, opens (sub_82D09998), screen vt+44(param), then the blur. The
  queue runner sub_82D0A2C0 waits while the top's state is not 0 / 3 and dispatches ops 1 pop, 2 push, 3 replace,
  4 pop to id, 5 close id.
- [code, research, not re-read] Op 5 below the top unwinds and reopens the id; pop shows a kept-open screen
  at once; the overlay queue runs only when the main one ran nothing; input goes to the main top only, not while
  main requests are pending. Record fields: +0 suspends gameplay (context 3 pushed on open, removed on close),
  +1 chyron, +8 HUD mode (0 on, 1 off, 2 online HUDs only, 3 keep), +12 leftover input to the movie.
- [data, verified] Field counts in TU3: +0 set for 52 screens, +1 for 19, +12 for 62, +8 = 1 for 64; the
  disc build has the same records minus TU3's ids 31 / 32 (test `disc_executable_gives_the_same_tables`).

### Change
- `skate-core/src/fe_stack.rs` (new): `FeStack` (main + overlay stacks, FIFO queues, the five ops, the wait rule,
  `input_target`), `ScreenInfo`, `Effect` for the host (open / close / show, gameplay suspend, chyron, blur, HUD,
  focus). Serialisable plain requests, screens by this build's index.
- `menu_tables`: `FeScreen` carries all record fields (`suspends_gameplay`, `chyron`, `hud`, `input_to_movie`).
- `retail_menu_stage.rs`: F8 pushes the pause screen (retail Start, plain push) or closes it (op 5); the crossbar's
  back and an engine row close it through the stack; input only while the stack says the pause screen has it.
  Gameplay suspend sets `graphics_menu::Menu::fe_suspended`, which `gameplay_active`, the time pause and the
  game audio mute already follow (multiplayer sessions never pause, as with the overlay). The menu and the blur
  run on real time.
- NOT RETAIL YET: HUD modes and the chyron are not hooked up; only the pause screen has a movie (other screens log
  "no template yet"); retail's focus filter for screen 30 is not ported.

### Verification
- `cargo test -p skate-core fe_stack`: pause push / close, the plain push closing the screen below and the pop
  reopening it, kept-open hide / show, close below the top, the overlay queue order, input gating.
- `cargo test -p skate-data --test menu_tables -- --ignored`: record fields and counts, disc vs TU3.
- A local screenshot: the pause menu through the stack, the world blurred and tinted,
  gameplay suspended (physics 0.01 ms), the selected row's glow animating.

## Pad input (2026-10-10)

### Problem
The dev view read four keyboard keys; retail turns pad input into Apt key indices through its input map and hands
them to the top screen's C++ handler and then its movie.

### Evidence (research note on FE input)
- [data, verified] The install's stock `input.cfg` defines all 56 Apt keys of the extracted key table
  (`AptStart = Start.pressed;`, `AptUp = LStickU.repeat || DPadU.repeat;`, `AptUpReleased = ...released`).
- [code, research] The FE poll sub_825DFDC8 evaluates the 56 keys in table order for every connected controller and
  calls the stack's input handler sub_825EC0E8 with the key index; that offers AptStart to the pause opener
  sub_825EB998, then the main top's C++ handler (crossbar sub_8261B560, which consumes only its close branch),
  then the movie's `ScreenManager.UpdateInput(id, key, controller)` when the screen's record +12 is set.

### Change
- `skate-core/src/fe_input.rs` (new): `parse_apt_keys` (Apt expressions over the 24 native slots, key-table
  order), `FeInput` (held / pressed / released / repeat edges per slot), `RepeatRule` (data), `fired`.
- `retail_menu_stage.rs`: every controller through `xbox::convert` (the gameplay conversion) plus the keyboard on
  controller 0's slots; AptStart opens / closes the pause screen; other keys go to the crossbar handler, then to
  UpdateInput unless consumed; input only while the stack gives the pause screen input.
  `SKATE3_RETAIL_MENUS_KEYS` injects key indices for screenshot checks. With the dev view on, pad Start no longer
  opens the overlay menu, and the overlay ignores Escape / Start while a retail screen holds gameplay.
- Repeat (updated the same day): retail's channel update sub_82966F30 (research `input-repeat.md` pass 2,
  constants verified): held above 0.5, 3 settle ticks after a change, repeat with the press, after 24 ticks,
  then every 12. NOT RETAIL YET: the tick rate is assumed to be the console's 30 Hz (0.8 s, then 0.4 s). Start
  closing the open pause menu is unverified.

### Verification
- `cargo test -p skate-core fe_input`: parsing in key order, unknown slots rejected, press / repeat / release,
  threshold, pressed keys do not repeat.
- A local screenshot (`SKATE3_RETAIL_MENUS_KEYS=13`): one AptDown skips the greyed
  rows to skate.Profile ("View your career profile"); the movie's UpdateInput after the handler changes nothing
  else.

## Game Settings screen (2026-10-10, in progress)

### Problem
The crossbar's Game Settings row opened our overlay menu; retail pushes FE screen 45, whose init opens template 90
"Options" (options.apt) and answers its `Game.GameSettings_*` / `Game.OnMenu*` natives from the settings tables.
options.apt also needs APT features the pause menu never used (attachMovie, component classes, Math).

### Evidence
- [data, verified] Screen titles: 13 string ids at TU3 0x83026E90 (0 / 1 ID_GAMESETTINGS_TITLE, 2 _DIFFICULTY_,
  3 / 12 _AUDIO_, 4..8 _VIDEO_, 9 _CONTROL_, 10 _ONLINE_, 11 _SKATEFEED_SETTINGS); the disc build has the same.
  FE 45's info record key `0ca9bfe5c1ba4995` (unique); its record suspends gameplay, shows the chyron, blurs.
- [code, local research notes] The natives, the loaded
  handler (SetScreenState 1), the begin-close handler (SetScreenState 2 outside FE state 2), Back (sub-screen ->
  main list, else close), Left / Right (sub_8260C258: step the row, SetState 50), select (sub_8260D290: links set
  the screen index and rebuild).
- [data, a local research note] main.apt's root defines RegisterComponentClasses and
  IncreaseScreenDepth as named functions on `_level0` and calls them by name (APT 0xb0); each control library
  registers its component class with `Object.registerClass("<Linkage>", class)`; highlight_option's constructor
  sets the default title "#TBD TITLE" and SetTitle forwards to its HighlightText child.

### Change
- `menu_tables`: `settings_titles` (located by two pointers to ID_GAMESETTINGS_TITLE).
- `settings_host.rs` (new): the GameSettings natives from the registry over a row snapshot taken at
  GetNumOptions; OnMenuSelect is queued for the host. Value descriptions: the Difficulty row only, from its value
  label (EASY -> DESCRIPTION_EASY ..., as retail's index table). NOT RETAIL (guard): when no row is enabled the
  first one counts as enabled, because options.apt's NextValueCalc recurses forever otherwise (mods can do that).
- `retail_menus.rs`: GameSettings now pushes FE screen 45 (`SCREEN_ITEMS`, owner `screen:<id>`) instead of the
  overlay section.
- `retail_menu_stage.rs`: opens / closes either ported screen by template (CrossBar 669 / Options 690 in TU3;
  the disc build numbers its 129 templates differently); settings input (Back, Left / Right) as sub_8260C258,
  steps and selects dispatched through the registry, then SetState(50) / SetState(1).
  NOT RETAIL YET: the per-row UI sounds (core_nav_left / right, core_a_button, core_b_button stand in).
- APT VM / stage (standard AS2 semantics, each marked NOT RETAIL YET in code): `attachMovie` (script depths above
  the timeline, kept across seeks, the subtree's class constructors run before it returns, nested clips first),
  `removeMovieClip` / `getDepth` / `getNextHighestDepth` for attached clips, call-by-name through the scope chain
  (locals, the timeline, then _global), `Math`, `TextFormat` (a record; setTextFormat not applied), `textHeight`,
  clip `_width` / `_height` from frame-0 bounds (clip-depth children left out like retail's bounds walk
  sub_82E59C28), library linkage names carried with imported copies (nested component classes).
- `SKATE3_RETAIL_MENUS_TRACE=1` logs the host calls the movies made (answered and unanswered).

### Verification
- `cargo test -p skate-game settings_host retail_options_screen_builds_rows` (the latter data-gated on the menu
  movie set): the natives in options.apt's order; the movie builds six rows from the registry and the first row
  reads ID_GAMESETTINGS_DIFFICULTY_SETTINGS.
- `cargo test -p skate-data --test menu_tables -- --ignored`: title table address and values, disc = TU3.
- A local screenshot (`SKATE3_RETAIL_MENUS_KEYS=15,15,15,2`): Game Settings with
  Difficulty / Audio / Video / Control / Online / skate.Feed Settings, the first row selected.

### Panel, sub-screens and colour (same day)
- [data, a local research note, verified] The squares were not gradients: panel.apt
  stretches 16 x 16 colour swatches (panelTopFill, panelCoreFill, ...) into its bands with `_xscale` / `_yscale`
  in BuildPanel, which our APT player ignored; and button_item2's `mButtonRender` is a placeholder for a native
  component (`_type = "CustomButton"`, `_test` = the button), drawn by the player.
- APT: `_xscale` / `_yscale` / `_rotation` seeded from the placement, script writes (and `_width` / `_height` on
  non-text clips, through their bounds) replace the matrix's linear part (`apt_movie::clip_matrix`, both matrix
  builders). The front-end stage host applies it; the gameplay HUD host does not yet (unchanged).
- removeMovieClip on attached clips reaches the movie (DeleteOptions on a sub-screen rebuild); test: screen 2.
- Colour: the menus apply colour transforms in gamma space (hud_render.wgsl mode 1): the console front end works
  on display values, and a recomp shot shows a near-black panel body where ours was mid grey (solid 0.145
  encoded twice). The gameplay HUD uses the same path and the scale support (user: follow retail; one APT player
  in retail). Checked: the line HUD's ring at 2 s is identical before / after (textured, identity transform);
  tinted / solid HUD elements now show their display colours (to look at in play).
- In game (`SKATE3_RETAIL_MENUS_KEYS=15,15,15,2,2,15`): A on Difficulty Settings opens the Difficulty screen
  ("Play Mode < Normal >", "Classic Skate Gameplay" description); Right steps the difficulty through the engine
  binding.

### HUD mode and chyron (same day)
- HUD mode (+8): the stack's Hud effect sets `fe_backdrop::FeHud`; the scoring HUD draws nothing and the online
  roster hides while the top screen's mode is "off" (the pause menu, the settings screen); an empty stack means
  "on" (retail's gameplay screen, FE id 0 mode 0, sits at the bottom; ours has no screen there yet).
- Chyron (+1): main.apt's layer movies are now exported (menu movie set 5: chyron, subtitle, fmv_popup,
  fadebackground, autosave; setup re-exports); the Chyron effect calls `_global.Chyron_SetVisible` on a change.
  NOT RETAIL YET: its text (Chyron_Display from the C++) is not driven, so nothing shows yet; PlayOutro not used.

### Open

## Button prompts (2026-10-10)

### Problem
Retail draws button glyphs through a native "CustomButton" component (Xbox 360 art). On PC the prompt must show
the connected pad's own button (user: the glyphs are replaced by SDL's matching button
prompt; size and placement as retail).

### Change
- `input/controller_kind.rs`: `PromptLabel`; `ControllerKind::face_labels` = SDL's labels of the South / East /
  West / North buttons for the pad type (`SDL_GetGamepadButtonLabelForType`, `input/platform.rs`), else the
  family's prompt style (XInput, unknown types).
- `apt_scene.rs`: a CustomButton clip with a face button in `_test` draws one quad over its placeholder's bounds
  (retail's box and placement, laid out by button_item2's own script) with texture `prompt/<button>`; other
  native components draw nothing.
- `prompt_glyphs.rs` (new): every retail button name (retail's list, TU3 0x82204614: A / B / X / Y, Back, Start,
  L/R_Bumper, L/R_Trigger, L3/R3_Click, Stick_Left / Stick_Right with _Up / _Down / _L / _R, DPad with
  _Up / _Down / _Left / _Right) has an equivalent on every family (user: every console family gets an equivalent
  button for every entry):

  | retail | Xbox 360 | Xbox One / Series | PS3 | PS4 | PS5 | Nintendo |
  |---|---|---|---|---|---|---|
  | A / B / X / Y | SDL label | SDL label | SDL label | SDL label | SDL label | SDL label (South = B) |
  | L/R_Bumper | LB / RB | LB / RB | L1 / R1 | L1 / R1 | L1 / R1 | L / R |
  | L/R_Trigger | LT / RT | LT / RT | L2 / R2 | L2 / R2 | L2 / R2 | ZL / ZR |
  | Start | START | MENU | START | OPTIONS | OPTIONS | + |
  | Back | BACK | VIEW | SELECT | SHARE | CREATE | - |
  | L3/R3_Click | LS / RS | LS / RS | L3 / R3 | L3 / R3 | L3 / R3 | LS / RS |
  | Stick_*, DPad_* | stick / cross with the direction marked, every family | | | | | |

Update 2026-10-18 (user: use the SDL glyphs for all button prompts): the same glyphs now replace the button names
in the engine's own UI too. `prompt_glyphs::PromptGlyph(<retail name>)` is a UI image whose glyph
`sync_ui_glyphs` keeps current for the first connected pad (re-resolved when the pad changes; one generated image per
prompt, `PromptImages`); `hint_row` lays out "glyphs, keyboard keys, action". Used by the Guide / F1 / Esc overlay
footer (A / B / Y / LB + RB / X; keyboard mode keeps its key chips), the replay panel (LT + RT, d-pad, A, Y, Back + B,
sticks, LB + RB, L3) and the mod menu (d-pad, A, B). Mods: the glyph art follows the prompt style table above.

  Face looks: Xbox coloured balls (retail's), PlayStation dark buttons with coloured symbols, Nintendo dark with
  light letters; named buttons a dark pill with the name. Generated (a small stroke font), no console art; sized
  to retail's measured glyph (0.83 of the box; a recomp shot, local screenshots). The menu
  renderer resolves `prompt/<button>` for the first connected pad (`resolve_for`), Xbox 360 without one.
  `cargo test -p skate-game prompt_sheet -- --ignored` with `SKATE3_PROMPT_SHEET=<folder>` writes a sheet.

### Verification
- `cargo test -p skate-game prompt_glyphs controller_kind`: glyph size / colours, SDL-less fallbacks (PS5 ->
  Cross..., Switch South -> B).
- A local screenshot: A / B prompts beside Select / Back like the retail shot.


## Sub-options (2026-10-10)

### Problem
GetSubOptionName answered "#" and the sub-option counts were empty, so no crossbar row could open its sub column
(Enter Skate Park, Skate Reel, On-Screen Help, ...).

### Evidence (research notes on the menu sub-tables and the CoreMenu host; table contents verified)
- [data] Label table TU3 0x83027958 and helper table right after it (0x83027A00): 7 kinds x 6 string pointers.
- [code] GetNumSubOptions sub_8261DB48: non-null labels of the kind's row; park kind minus one unless global
  [0x830B7AE8] == 1; online kinds by online state. GetSubOptionName sub_8261DE88: the label by (kind, sub), with
  on-screen help naming the action (OFF when on). Level-2 helper text from the helper table (sub_8261E270).

### Change
- `menu_tables`: `sub_options` (`SubOptionRow { labels, helpers }`, located by the pointer to ID_CROSSBAR_NEW_PARK);
  setup re-extracts (menu_tables.py requires it).
- `crossbar_host.rs`: `sub_option_counts` (the count rule; facts `crossbar.park_tutorial`,
  `crossbar.trick_analyzer`, `crossbar.manual_meter` for the flags), GetSubOptionName, level-2 helpers; the stage
  builds the crossbar view with these counts. NOT PORTED: the online kinds' rules (offline counts: contacts 3,
  online team 3, party 1) and label swaps.

### Verification
- `cargo test -p skate-data --test menu_tables -- --ignored`: TU3 table address and rows, disc 7 rows.
- `cargo test -p skate-game crossbar_host`: counts (park tutorial flag), names, out-of-range "#", level-2 helper.

## PC overlay on the Guide button (2026-10-11)

### Decision
The user suggested a PC overlay like the recomp's instead, opened with the guide button. PC-only features
(Mods, Updates, custom models, day & night, local testing, network debug, mod sections, custom maps, PC video /
audio / control settings, Quit) live in a PC overlay like the recomp's settings overlay (rexglue
`simple_settings_overlay`: a fullscreen console-style screen, category rail Video / Controls / Audio / Profile /
System, opened by its own binding). The retail menus stay retail: Start opens the retail pause menu; retail's
Extras keeps its own rows.

### Change
- `input/platform.rs`: SDL's Guide button reads as bit 0x0400 (XInputGetStateEx's place; retail's 24-slot
  conversion never reads it; not a bindable name for paddles).
- `graphics_menu.rs`: Guide, F1 or Escape toggle the overlay from anywhere, also over the retail menus; pad Start
  opens it only while the retail menus are off. `retail_menu_stage.rs`: no retail key handling while the overlay
  is open.
- The XInput fallback backend reads Guide too (2026-10-29): `input/platform.rs` polls through XInputGetStateEx
  (xinput1_4 ordinal 100, the same state layout with Guide at 0x0400, as SDL does) when it exists, else
  XInputGetState; input tests 50 pass, not checked with a pad on that backend. Open: Windows Game Bar may take the
  Xbox Guide press first (its own setting).

### The recomp's look and navigation (2026-10-11)
The user's decision: faithfully recreate the recomp's guide menu and replace ours with it. The overlay's view is now a port of
the recomp's settings overlay (skate3recomp's rexglue-sdk, `src/ui/overlay/simple_settings_overlay.cpp`,
`SimpleSettingsDialog::OnDraw` / `GatherInput`, at `third_party/rexglue-sdk` commit ae6ea8e); the rows stay ours.
- Look: the recomp's palette, metrics (authored for 1080p, `s = clamp(h / 1080, 0.6, 3) * 0.92`, legend at the
  base scale), column layout (300 s rail and description, content clamped to 300 to 800 s, the block centred, a
  whole number of 52 s row slots, at most 12), title "Settings" (sub-pages show their own name), lime section bars,
  white rows, magenta steppers that grey out at the ends, magenta sliders, the black focus fill with a 2 s lime ring
  and a 4 s black edge outside it, the teal rail with Close Game pinned under it, the Description panel, and the
  pad / keyboard legend that follows the last input. Font: Inter Regular / SemiBold (SIL OFL, subset to Latin,
  `crates/skate-game/src/fonts/`, from Inter 4.1), the recomp's typeface; sizes converted from ImGui line heights
  to Bevy ems (2048 / 2478).
- Motion: the focus fill slides at the recomp's rate (`min(1, dt * 22)`, snapping beyond 160 s), the list scroll
  locks to whole rows and slides at `dt * 16`, focused text styling waits until the fill mostly covers the row, held
  directions repeat after 0.42 s every 0.11 s, the wheel steps whole rows.
- Navigation: focus zones rail / rows; Up / Down on the rail switch category, Right or Select enters the rows;
  Left / Right step (clamped), Select cycles with wrap or runs the row; B / Escape back out one level (text edit,
  sub-page, rows, rail, closed); F1 / Guide close at once; LB / RB or Q / E (and Tab) change category; Y / R reset
  the row to its default; X / F apply pending custom tuning (the recomp's Apply & Restart slot; its chip reads
  "TUNING NOT APPLIED"). The mouse hovers rows into focus, clicks the rail, steppers, rows and slider tracks.
- Backdrop: the recomp blurs the frame (gaussian sigma 9 px at 1080p, eased with a 60 ms time constant) under a
  `(6, 9, 11, 133)` wash. Ours uses `fe_backdrop`'s pass at strength 12 (= sigma 9 at 1080p for that shader) with the
  same easing, and the wash as the overlay's background. The gameplay HUD is hidden while the overlay shows (the
  recomp's HUD is part of the blurred frame; ours draws above the blur pass).
- Deviations, not the recomp's: description text is left-aligned (the recomp justifies only when gaps stay under
  1.6 spaces; Bevy's justify has no such limit and opened wide gaps); value text clips instead of shrinking then
  ellipsizing; hover needs 2 px of movement since the last hover instead of per frame; Reset to Default shows in
  the legend only on rows that have a default; Close Game replaces the Extras "Quit game" row.
- NPC Draw Distance (Graphics > Living World; QoL, not retail): the living world's row (None, Retail, 1.5x, 2x,
  3x), saved in `graphics.json` as `npc_draw_distance`. Here it is a placeholder: the overlay keeps the choice in
  the `NpcDrawDistance` resource, which nothing reads until the living world (`world/living-world`) lands and
  applies it; its description says so. On `hails-additions` the row drives the living world directly.
- Moddability: mod menu sections still become rail categories (`custom_sections`), their entries rows.
- Tests: `cargo test -p skate-game graphics_menu` (keys, back steps, stepper ends, header placement, NPC draw distance setting, repeat timing,
  1080p layout, easing).
- Open: the user's play test of the motion.

## Swap-in: the retail menus are the default (2026-10-11)

### Change
- `retail_menu_stage.rs`: the stage runs unless `SKATE3_RETAIL_MENUS=0` (was: only with `=1`). Start (and F8) open
  the retail pause menu; Game Settings opens the retail settings screen; its sub-screens step the bound rows.
- `RetailPause` (resource): true while the stage loaded and the menu tables carry the FE screen records. When it is
  false (tables extracted before those records, missing movies, a failed stage, or the menus off), Start opens the
  PC overlay instead, the fallback, with one log line: `the menu tables have no FE screens yet (run setup); Start
  opens the PC overlay until then`. `graphics_menu::overlay_key` takes that flag instead of the env switch.
- The PC overlay keeps Guide / F1 / Escape in both cases.

### Verification
- Default (no env flags, fresh tables): `SKATE3_RETAIL_MENUS_OPEN=1` opens the retail pause menu ("Free Play >
  Main", Challenge Map selected).
- Fallback (the dev install's tables from before the FE screen records): the one warning above, no retail screen.
- Idle cost (gameplay, menu closed, 1280 x 800, frame counter at 14 s): on 2.74 / 2.10 / 2.08 ms, off 2.12 / 2.13
  / 2.08 ms; the first "on" run is the only outlier, the other two match "off" to 0.02 ms.
- `cargo test -p skate-game`: 508 passed, the known `pipelines_accept_valid_group_outputs_when_fingerprint_changes`
  failure.
- Play-tested as the default in hails-additions sessions on 2026-10-12 (pause menu freeze, settings text colours,
  Trick Guide; each fixed in its own section below).

## Stored settings: every rule row steps and saves (2026-10-11)

User decision: store first, then features: first make every settings row work as a setting (steps, saves, readable),
then port each row's effect. Rows that save but have no effect yet are listed below.

### Change
- `retail_menus.rs`: `StoredSettings` (`settings/game-settings.json`, string id -> number). Every settings row with a
  retail rule and no engine binding (`BINDINGS`) gets an `Owner::Engine("settings")` value layer: the saved value,
  else retail's default. That enables the row; Left / Right step it through retail's rule; a change is saved. A
  mod's value layers on top and is never saved (same as the engine bindings). `retail_menus::setting(menus, id)`
  gives features the value in force.
- Effects so far (a local research note; verified the parts marked so):
  - HUD +164 [code]: retail `sub_825DA2A0` shows / hides the HUD clips trickdisplay, minimap, central and
    trickanalyzer (manualmeter and online_chyron in the HUD setup `sub_825D99C8`; homscoring always on). Our
    gameplay HUD draws only trickdisplay, so Off hides it (`scoring_hud.rs`); the other clips follow when they are
    ported.
  - Offboard X / Y axis +193 / +192 [code, verified]: `sub_8252FEB0` packs them into the camera config
    `[0x83085450]` +24 / +25 (Transparency +195 into +26); the offboard camera `82DF69C0` negates the look stick's
    x when +24 is set and y when +25 is set. Ours: `camera::angle::sync_look_invert` sets
    `CameraRuntime::invert_look`, which the existing publication applies to `look_552_556` (the right stick).
    Test `look_invert_follows_the_axis_rows`. Not play-tested.
- Vibration +163 (2026-10-11): rumble output ported (a local research note; constants verified
  in the TU3 image, the tick and device curve verified in `sub_82695010` / `sub_8296D480`):
  - `skate_core::rumble`: per pad, channels {low, high, decay}; a start overwrites its channel with low / high
    clamped to 0..1 and decay = clamp(1 - rate / 60) (`sub_82694D38`, only it checks the vibration flag, so
    switching off lets running rumble fade); each tick sums the channels per motor, caps the sum at
    max + (1 - max) * 0.2, clamps to 0..1, then decays each channel and drops it once both motors are below 0.001
    (`sub_82695010`); the device curve (`sub_8296D480`): wireless pads (capability type 1 and flags & 0x0002 =
    X_INPUT_CAPS_WIRELESS, `rlwinm r10,r11,0,30,30`; verified 2026-10-17) motor 0 = 1.2 l - 0.02, motor 1 =
    1.05 l + 0.05, wired and other pads 0.7 l - 0.02 / 0.7 l, clamp, exactly 0 stays 0, else trunc(v * 65535). As in
    retail, a wireless pad's motor 1 at level 0 gets 3276 (0.05). Values in `RumbleTuning::RETAIL`.
  - `rumble.rs` (game): ticked at 60 Hz of real time whatever the frame rate; the Vibration row (value in force)
    gates starts; switching it on plays retail's test rumble (`sub_8260DF40`: channel 0, 0.5 / 0.5, rate 5.0) on
    the active pad; output through `input::platform::set_rumble` (SDL `SDL_RumbleGamepad`, re-sent every 0.5 s
    for a 1 s duration while non-zero; XInput `XInputSetState` on change). The curve follows retail's test: wireless
    from XINPUT_CAPS_WIRELESS (XInput) or SDL's connection state (`SDL_GetGamepadConnectionState`); a pad SDL
    opened through its XInput driver (path `XInput#<slot>`, connection state unknown) reads XINPUT_CAPS_WIRELESS of
    that slot (2026-10-17: the laptop's Elite 2 came up unknown, so wired); still unknown counts as wired.
  - Fix 2026-10-17 (user: controller rumble still started as soon as the game did and did not
    stop when the game closed; research b173): before, every Xbox-family pad took the wireless curve and the
    curve ran with no live channel, so motor 1 got 3276 from the first frame, SDL re-sent it every 0.5 s, and no
    exit path sent a zero. Now: NOT RETAIL (host) an idle pad (no live channel) gets [0, 0] instead of the curve's
    floor (retail sends the floor and the console driver lets it lapse; a PC pad keeps it); zero is never re-sent;
    on exit `platform::stop_rumble_on_exit` zeroes every pad after `app.run()` returns (window close, quit, Ctrl+C)
    and in the panic hook (SDL thread answers within 200 ms, plus all XInput slots on Windows); the crash supervisor
    zeroes all XInput slots once the game process ends, which covers a killed game. Tests: core curve (wired has no
    floor), `an_idle_pad_gets_a_stop_and_a_live_one_the_retail_curve`, `only_a_wireless_pad_takes_the_wireless_curve`,
    `idle_is_never_refreshed_and_a_running_rumble_is`. Not felt on a pad yet.
  - Mods: `sdk.input.rumble` / `stop_rumble` (capability `input_rumble`; per-mod channels, stopped with the mod).
  - NOT RETAIL (2026-10-17, user: default it to off for now): Vibration defaults to Off (retail +163 = 1) until
    the rumble that starts with the game and keeps going after it closes is fixed (todo `rumble-starts-and-sticks`);
    with no setting found it is off too. Switching it on in Game Settings works and is saved.
  - NOT PORTED: retail's gameplay rumble events (the posters `sub_82BB8CF0`, 0.5 / 0.5 / rate 1.2, local player only,
    and `sub_8259E3D0`, per-item channels from `sub_8259DFB0`; which events they belong to is not proven; a recomp
    trace hook on `sub_82694D38` logging caller, channel and the three floats would settle it).
  - Tests: `cargo test -p skate-core rumble`, `cargo test -p skate-game rumble` (60 Hz at any frame rate, the
    switch-on test rumble, off blocks starts, mod channels separate and stopped with their mod). Not felt on a pad.
- No effect yet, so greyed (2026-10-13, see "Settings without an effect are greyed" below): Minimap +158
  (`sub_82665278`: hidden with the HUD off, forced on / off in some game states; we draw no minimap), Subtitles +157
  (six readers gate subtitles on it or a forced flag; no subtitle text in the engine), Cameraman Mode +165 (decoded
  2026-10-13, see "Cameraman Mode" below; not ported), HOM mode +184 (readers found, effect undecoded), Units,
  Transparency (no reader of the copied byte found), the Skate Feed toggles.
- Still greyed (no retail rule decoded): SFX Pack (hidden with one pack), Auto sign-in (an online popup; decoded
  2026-10-13: Left / Right flip byte +194, default 1, a popup only when switched off; left out by the user's choice).
  Video Calibration now opens its screen (below).

### Verification
- `cargo test -p skate-game stored_rows_are_enabled_step_save_and_reload_and_mod_values_are_not_saved`: a stored row
  is enabled with retail's default; nothing is written until a change; Right saves Off; a mod's value shows and is
  not saved; a new session loads Off; engine-bound rows are not stored.
- In game (fresh tables, `SKATE3_RETAIL_MENUS_KEYS=15,15,15,2,13,13,2,13,13,13,13,15`): Video Settings, HUD selected
  and stepped to Off; log `ID_GAMESETTINGS_HUD_TOGGLE = Int(0) (saved)`, the file holds `{"ID_GAMESETTINGS_HUD_TOGGLE": 0}`.

## Session logging (2026-10-11)

The user asked for proper logging of the new living world and retail menu changes, so their sessions can be
reviewed from the logs. Every line is `info!`, so `PLAY-WITH-LOG.bat` sessions carry them; grep by prefix:
- `RETAIL_MENUS key <AptKey> pad <n> screen <id>`: every key the retail menus took (retail's Apt key names);
  `RETAIL_MENUS open / close pause menu`, `open / close Game Settings` (FE ids), `gameplay suspended / resumed`,
  `settings back to the main list from screen <n>`, `select <item>: <outcome>`, `settings step <id> <dir>: <outcome>`,
  `<setting id> = <value> (saved)`, the fallback warning `no FE screens yet (run setup)`.
- `PC_OVERLAY open / close by <Guide | F1 | Escape | Start>`, `back by <Escape | B>`, `at <category> / <page>`,
  `<Step | Activate | Reset>(row) "<label>" was "<value>" (section, page)`, `now "<label>" = "<value>"`,
  `status: <message>` (saves and errors).
- `RUMBLE Vibration on / off`, `settings test rumble on pad <n>`, `mod <id> pad <n> channel <c>: low high rate`
  (and "not started" while Vibration is off), `mod <id> stopped`, `pad <n>: wireless / wired curve (<pad summary>)` on connect, `pad <n> rumbling / idle (speeds
  <low> / <high>)`, `stopped on exit (<backend>)`; the controller summary says `wireless` / `wired` for SDL pads.
- Already there: `Offboard look invert: x y`, `Camera angle: <label>`, `PC overlay toggled by the Guide button`.
- Checked in a scripted run (keys 15,15,15,2,...): Vibration on, AptStart, gameplay suspended, open pause menu,
  three AptRight, AptNext, select, close pause menu, gameplay resumed / suspended, open Game Settings.

## Out-of-date game files prompt (2026-10-11)

The user asked for a prompt in the new guide overlay that asks players to refresh their assets when they are out of date.

### Problem
A release lists the asset converter versions it expects (`release.json` `asset_pipelines`). When a converter changed
but the group's recorded outputs were still present, `setup::asset_root` accepted the old files and stamped the new
versions as installed, so nothing ever asked for a refresh. That is how an install kept menu tables from before the
FE screen records (Start then falls back to the PC overlay).

### Change
- `setup.rs`: `outdated_groups` lists the groups with no exact or equivalence match at startup (`stale_groups()`,
  logged as `SETUP: game files out of date for <groups>`); `stamp_pipelines` keeps those groups' old fingerprints, so
  every start asks again until a refresh. `--refresh-assets` makes a running copy go through setup's refresh;
  `restart_with_refresh` starts the exe again with it (needs `support/skate3setup.exe`, the release package's).
- PC overlay: while groups are stale, the title chip reads "GAME FILES OUT OF DATE", every description ends with
  "Your game files are out of date: Extras > Refresh Game Files updates them.", and Extras gets a "Game Files"
  section with **Refresh Game Files** (describes the stale groups by name; Select restarts into setup, logged
  `PC_OVERLAY refresh game files: restarting into setup`; without a setup helper the status says so).
- The user's decision: do not trust a flag for asset staleness; a lightweight checker runs on each launch
  and tests it. `asset_check.rs` runs on a background thread at every start and reads the files
  themselves: every output in each group's setup receipt (`installation.json` `outputs`) must exist at its recorded
  size (no hashing), and the HUD group must hold what this build reads (menu tables parse in this build's reader and
  carry the FE screen records, settings titles, sub-option tables and screen templates; the menu movie manifest is
  this build's format version and every file it lists exists). Each failure is logged `ASSET_CHECK <group>: <reason>`
  and marks the group stale; `ASSET_CHECK done in <ms>: game files current` otherwise (178 ms on the refreshed dev
  install). Groups other than HUD are checked by their receipts; content checks for them (for example the living
  world's data) are added with the features that need them. Before restarting into setup, Refresh Game Files writes
  "stale" as the version of every stale group in `installation.json`, so setup's refresh rebuilds exactly those.
  Tests `a_missing_or_resized_output_flags_its_group`, `hud_check_reads_the_files_not_a_stamp`.
- Development roots (`--assets`, `SKATE3_ASSETS`) have no `release.json`, so the version check never fires there;
  `SKATE_VERIFY_MENU=stale` previews it for screenshots.

### Verification
- `cargo test -p skate-game changed_converters_are_reported_and_keep_their_old_fingerprint_when_stamped`.
- Screenshot (`SKATE_VERIFY_MENU=stale`): chip, Game Files section, the row and both description lines.
- The restart into setup was run end to end by the user (2026-10-29).

## Refresh Game Files in development copies, and which files differ (2026-10-12)

### Problem
A Steam launcher copy of hails-additions showed GAME FILES OUT OF DATE (living world), and Extras > Refresh Game Files
only said "This copy has no setup helper". The log said `ASSET_CHECK livingworld: converted files missing or changed
size` without naming a file.

### Root cause
- The 19 car models in the dev installation had been re-exported straight into it during the car shader work
  (2026-10-11, 18:47), after setup's last receipt (15:22): each file 108 bytes larger than recorded. The game data was
  current; the receipt was not. Release copies are not affected (the converter change shows as a normal refresh offer).
- The refresh restart only knew the release layout (`support/skate3setup.exe` and `data/` beside the exe). Development
  copies run `bin/skate3rust.exe --assets <link>`, so there was no helper and the wrong data folder.

### Change
- `setup.rs`: with `--assets`, Refresh Game Files finds the installation behind the link (`<base>/installations/<id>/assets`),
  clears the stale groups' fingerprints there and restarts; the restarted copy runs setup's refresh (the packaged helper
  when present, else the checkout's `tools/setup.py` through `py -3.13`, the source the helper is built from) and points
  the link at the installation setup publishes. Only a link is replaced, never a real folder.
- `asset_check.rs`: the receipt finding names the first three files that differ and the count, e.g.
  `converted files missing or changed size (19 of 114: .../suv_01.glb (710100 bytes, recorded 709992), ... and 16 more)`.
- Launcher (`.local/steam`, not in the repo): a broken version link is re-pointed at the installation in
  `data/installation.json`, not at the main checkout's link (which can be stale as well).

### Verification
- `dev_refresh_finds_the_base_and_repoints_the_link` (Windows): the base behind a junction, re-pointing to a path joined
  with '/' (mklink read it as a switch before the fix), the old target kept, a real folder refused.
- `a_missing_or_resized_output_flags_its_group` checks the new reasons.
- The dev installation refreshed with setup's `refresh=True` (only `livingworld` changed): new installation, every
  group's receipt matches (core 2033, hud 27, character 60, environment 38, maps 26, audio 4416, livingworld 115 files).
- Run end to end from the overlay button in a development copy (the user, by 2026-10-13).

## Opt-in features in the PC overlay (2026-10-11)

The user's decision: every opt-in gets a setting in the guide overlay that toggles it on and off, where it does not have one yet.

### Problem
Two player-facing opt-ins on this branch had no overlay row: the non-retail "more audible" world layout (only
`settings/audio.json` `"more_audible_world": true` by hand, or `SKATE_AUDIO_MORE_AUDIBLE=1`) and setup's speech
decode (`SKATE_SETUP_SPEECH=1` in the environment of a setup run, about 3.4 GB for the world, main cast and
announcer speech). Without the decode, ped speech is chosen and logged but silent.

### Change
- PC overlay, Graphics: a new "Opt-In Features" group after Living World (header 913) with **More Audible World**
  (row 32: Retail / On, saved in `settings/audio.json`, takes effect at the next start because the runtime picks
  its instance counts at launch; Y / R resets it to Retail) and **Ped Speech** (row 33: Off / On, saved as
  `decode_speech` in `settings/audio.json`). The living world branch adds its opt-ins (zombie mode, NPC skater
  physics, skitching, prop grab) to the same group where both branches meet (doc 26b "Opt-in switches in the PC
  overlay").
- Ped Speech on without decoded takes (`private/audio/speech/livingworld` empty or missing) marks the `audio`
  group stale, so Extras shows **Refresh Game Files** and the title chip says GAME FILES OUT OF DATE; the same
  check runs at every start while the choice is on (a cancelled refresh asks again). `setup::asset_root` passes
  `SKATE_SETUP_SPEECH=1` to the setup helper when the install's saved choice is on (logged
  `SETUP: speech decode asked for`). Off keeps the decoded takes; setup skips the decode from the next refresh.
- `SKATE_VERIFY_MENU=optin` opens the overlay on the group for screenshots.
- 2026-10-17, user: opt-ins move out of Graphics into their own section of the guide overlay. The
  group is now its own rail section, **Opt-Ins** (key `OPT_INS`, after Extras, section 5), with the same rows and
  header 913; Graphics ends at Living World. Appended rather than placed next to Graphics because Multiplayer (3)
  and Extras (4) are addressed by index. Test: `sections_expose_only_real_rows_and_all_maps`,
  `pages_put_headers_in_front_of_their_groups`. `SKATE_VERIFY_MENU=optin` opens the new section.

### Verification
- `cargo test -p skate-game --bin skate3rust -- graphics_menu game_audio::tests setup`: all pass except the known
  upstream failure `pipelines_accept_valid_group_outputs_when_fingerprint_changes`. New test
  `speech_decode_choice_reaches_setup`.
- Screenshot on DownTown (`SKATE_VERIFY_MENU=optin`): the group header, both rows and the Ped Speech description.
- Found while taking it: with `--test-world` the overlay capture (`SKATE_VERIFY_MENU=optin` and the older `npc`)
  panics in wgpu ("fe backdrop" pipeline Rgba16Float vs the Rgba8UnormSrgb pass); on a real map it renders. Not
  caused by this change; open.
- The refresh with the decode was run end to end by the user (2026-10-29).

## Pause menu froze the game on the third open (2026-10-12)

### Problem
A hails-additions session (Steam launcher, trace-all) froze with the pause menu: the game stayed suspended and Start did
nothing, so the game had to be closed.

### Root cause
- The front-end stage never collected its VM objects. Every pause open loads the crossbar movie (about 1300 objects);
  closing it unloaded the level but the objects stayed, so the third open crossed the VM's 4096-object guard
  (`RETAIL_MENUS stage stopped: APT execution/object budget exceeded`).
- `Vm::collect` existed but nothing called it; and when called, the stage's table of script-made clips
  (`createEmptyMovieClip`) kept every closed screen alive through the clips' parent links.
- A stopped stage left gameplay suspended (`fe_suspended`) and the pause backdrop up, and Start is ignored while
  suspended: the freeze.

### Evidence
Session log `game-20261012-072425.stderr.log`: pause opened three times (two Challenge Map -> PC overlay round trips in
between), the third open fails 5 ms after `open pause menu (FE 54)`, physics tick stays at 392 to the end.

### Change
- `apt_stage.rs`: `Stage::collect` runs at the end of an update once the slots reach twice what survived the last
  collection (at least 1024). Roots: `_global`, the stage's prototypes, classes and pending loads, and every object id
  the loaded movies hold (root, instances and children, queued actions, classes, script-moved / attached / scaled
  clips). The script-made clip table is weak: entries go when their clip is collected.
- `apt_vm.rs`: `collect` returns the free slots; the object guard is 16384 (a runaway guard, not a retail limit).
- `retail_menu_stage.rs`: a stopped stage releases the gameplay suspension, the pause backdrop and the HUD mode once
  (warning `stage stopped with the pause menu open: gameplay resumed`); Start then opens the PC overlay.
- NOT RETAIL (mechanism): retail's APT player refcounts its objects; the collection only keeps the slots bounded and
  changes no script-visible behaviour (only unreachable objects are freed).

### Verification
- `cargo test -p skate-game pause_menu_reopens_without_growing` (data-gated on the exported menu set): 8 open/close
  cycles of the real crossbar through the retail screen manager; live objects after each cycle
  `[908, 908, 908, 908, 908, 908, 908, 908]`. Before the change it failed on the third open with the session's error.

### Open questions
- Challenge Map still opens the PC overlay's Maps section (the bridge in "Row handlers"); retail's Challenge Map
  screen (FE 17) with its locations and teleports is not ported.

## Settings text colours and the glow (2026-10-12)

### Problem
Play test: on Difficulty Settings the current value ("< Normal >") was unreadable, the Audio "100%" values were very
dim, and the selected row's glow was far too bright while it flickered.

### Root cause
- [data, code] The rows' own scripts set `TextField.textColor` per state in SetState (highlight_selector,
  highlight_slider, highlight_option, menu_part_text_hilite, main: selected value 0x37A5FF, rows on 0x70BBFF, off
  0x385570, disabled 0x555555). Our text draw never read it and used only the field's authored `color_argb` (dark
  navy #0F4670 for the selector value, grey #3E485C for the slider value, #70BBFF for every row title).
- [code, medium-high] The HUD / menu layer blended in linear space (target `Rgba8UnormSrgb`, shader output converted
  to linear). Retail's APT draw state is straight SRC_ALPHA / INV_SRC_ALPHA (`sub_827E4AF0`, bound by the draw
  `sub_825D4490`) on a display-value image (the 2D pass's format is inferred from the recomp's tone chain and frame
  surface, not traced). Linear blending lifts the selected row's many soft low-alpha glow layers.
- Research: a local note (verified the textColor finding and the draw path).

### Change
- `apt_scene.rs`: a text draw takes the RGB of the field's script-set `textColor` when set, the alpha from the field.
- `scoring_hud.rs`, `hud_render.wgsl`, `hud_composite.wgsl`: the layer target is `Rgba8Unorm` holding display values;
  both shader modes output display values (mode 1 applies the colour transform to the texel's display value); the
  composite undoes the coverage, converts to linear and premultiplies again for the frame. The gameplay HUD shares the
  layer (one APT player in retail).
- Moddability: the colours stay in the movie scripts (mod movies and scripts use the same path); no Rust constants.

### Verification
- `retail_options_screen_builds_rows` now also asserts the script colours (0x385570 / 0x37A5FF) reach the draws; the
  full skate-game suite passes apart from the known upstream `pipelines_accept_valid_group_outputs_when_fingerprint_changes`.
- In game (local test build): the menus load (a first build put a helper between `@fragment` and the entry point and
  the composite shader failed to compile; fixed). The user's visual check of the glow and the gameplay HUD is pending.

### Open questions
- Prove the 2D pass's colour format with a short recomp trace at `sub_825D4490`.
- `apt_scene.rs` ignores `blend_mode`; retail also has an additive ONE/ONE object (`[0x830680AC]`, user not traced).
- Trick Guide (Learn tab): ported, see "Trick Guide".

## Trick Guide (2026-10-12)

### Problem
Pause menu > Learn > Trick Guide was greyed: no handler and no retail screen (play test 2026-10-12).

### Root cause
Not ported yet. Retail runs it as FE screen 14 (template 29 TrickTutorial, movie `screens/tricks/trickguide`); the
C++ screen object answers the movie's FETrickTutorial natives and its picker callbacks from the trick records.

### Evidence (TU3; every handler named here re-read)
- Open: crossbar case 0x8261BF78 starts the fade hold `sub_825E6B30`; its callback 0x8261CDD8 opens FE screen 14
  through `sub_8261AF30` (the same path as Game Settings / FE 45). Screen 14's vtable is 0x823038B8; init 0x82618A40
  opens template 29.
- Lists (`sub_82619368`): level 0 = the 8 category keys at 0x830274B0 (static); level 1 = the category's RefSpec array
  0x57035D975F71D506 (subcategories); a level with fewer than 2 entries is skipped straight to level 2; level 2 = every
  `scoring_trick` record with a demo animation (Text 0x3B71EA27D53D7C4F) whose subcategory ref (0x06513CC28A532524)
  matches, sorted by the Int32 at layout offset 128 (field 0xF9E5D2F69066E925; insertion / intro sort, ascending).
  S+128 = !Bool 0xA990472B974734C3 of the new list's first subcategory: leaf subcategories show their own slots.
- Natives (registrar `sub_825AE7E0`): GetMenuItemData 0x825AE8A8 = [level, has submenu (`sub_826192A0`: level 0 true,
  level 1 the subcategory's bool, level 2 false), description (vt+88 0x826191B0)]; GetMenuItemGesturesButtons
  0x825AE968 = [slots with a gesture or button set, then gesture name, button name, connector per slot] (gesture ids
  58 / 59 read ""; connectors 1 Plus, 2 Arrow, 3 Or); IsGoofy 0x825AEBF0 always false; GetCurrentMenuItem = S+116.
  Game.GetMenuItems 0x825A52A8 lists the current screen's vt+80 / vt+84 (level names: category / subcategory name,
  trick label). Game.OnMenuGetItemEnabled goes to vt+72 = 0x8281DD70 (always true).
- Picker callbacks: OnMenuHilight vt+68 0x82618E98 (row; SetState(10) on the upper levels, else fill the slots
  `sub_82619D38` from the trick or, with S+128, the subcategory record, then SetState(11)); OnMenuSelect vt+64
  0x82618D98 (save the row, open the next level, an empty level goes back, SetState(1)). Input vt+40 0x82618F38: Back
  closes at level 0 (fade, callback 0x82618EF0) or goes up a level, twice past a one-entry level, clears S+128,
  SetState(0), plays core_b_button; A on a row without a list is consumed.
- Slots: `Sk8::FE::tTrickGuideInputSlot` arrays (12 bytes: gesture id, button id, connector); name tables 0x830273C8
  (58 gesture names) and 0x83027350 (28 button names). Button remap `sub_82619FC0`: when exactly one of CAS byte +120
  (= HUDComponents.PlayerStance_IsRegular 0x826635A0) and a controls byte is set, the triggers swap, A shows X, X shows
  A or B; the movie swaps A / X and the triggers back for a regular stance.
- Fonts: the movie's title font "HelveticaPresto" has no FontManager row; `sub_82809208` then takes the first row
  whose FileName is not "debug" (`sub_82AE8A10` returns 0 on a match).
- Word wrap (the description field): `sub_82CA14D0` loop 0x82CA1BB0 breaks greedily at spaces only, `^` forces a
  break, a word wider than the field overhangs; lines advance by vfunc +72 (0x82806470: font+360 x scale) x font+364
  (FontManager row +24 = field 0x672E35A2AA8C4A00: futuraheavy 0.9, others 1.0) x height; per-line alignment
  0x82CA23B4 (1 right, 2 and 3 centred); no clipping.
- Research: a local note, `trick-guide-gestures.md`, `apt-text-wrap.md`.

### Change
- `skate-core/src/trick_guide.rs` (new): the data model (records by Attrib key), the navigation (lists, select, back,
  highlight, slots, the button remap) and mod patches (`EntryPatch`, `TrickGuide::apply`).
- `skate-data/src/trick_guide.rs` (new): builds the guide from the installed collections; `menu_tables.rs` extracts
  `trick_guide` (category keys by the code that copies them, the two name tables by content); `collections.rs` keeps
  array elements (`items`).
- `skate-game/src/trick_guide_host.rs` (new): the natives and picker callbacks, `TrickGuideMods` (mod layers);
  `retail_menu_stage.rs`: FE 14 open / close, input, queued screen states, the TrickGuide item's engine handler (only
  with tables, movie and collections, else the row stays greyed); `crossbar_host.rs` routes the objects;
  `fe_backdrop.rs` `TRICK_GUIDE_SCREEN`; `skater_animation.rs` `regular_stance`.
- APT fixes found on this screen: `apt_movie.rs` `class_links` (every copy of a library symbol gets its registered
  class; the screen imports "Button Item" itself and again through panel > button_menu, so its button help had no
  SetButtonType and no prompts); `apt_text.rs` / `apt_scene.rs` word wrap, line advance and alignment 3 centred;
  `apt_movie.rs` `link_imports` (2026-10-13: every library a screen copies from keeps all its exported symbols
  reachable by linkage name; gesture_item's `SetGestureType(gesture)` runs `attachMovie("g_" + gesture,
  "gestureitem", 0)` on symbols its timeline never places, so the stick icon showed the ring with no arrow. Retail
  draws one static arrow per trick, checked frame by frame in a PS3 recording. NOT RETAIL YET: retail's
  attachMovie lookup is not decoded, this follows the Flash rule).
- Export: `prepare_menu_movies.py` adds the trickguide root and the FontManager fallback for APT names without a row;
  `prepare_hud.py` exports `LineSpacing`; `menu_tables.py` requires the trick guide tables (setup refreshes both).
- Moddability: `sdk.menus.trick_guide(entry, key, options)` adds or changes categories, subcategories and tricks by
  key (retail 16 hex digits or the mod's own id): label, description, up to 4 slots, animation, parent, order,
  has_tricks, position, hidden; layered in mod order, removed when the mod stops (`skate-mods` command
  `menu_trick_guide`, `api.lua`, `sdk/skate.lua`). Retail values are the defaults, all from setup data.
- Multiplayer: local UI state only.

### Verification
- `cargo test -p skate-core trick_guide` (navigation, skip rules, leaf slots, remap, patches); `cargo test -p
  skate-data --test menu_tables -- --ignored` (TU3 image and disc xex give the same tables: 8 categories, 58 gestures,
  28 buttons); `--test trick_guide -- --ignored` walks the real tree (8 categories, 217 listed tricks; Kickflip's slot is
  ollie_kickflip); `retail_trick_guide_screen_opens` (screen manager, picker callbacks, B prompt drawn, Ollie Flips
attaches the `g_ollie` arrow to the gesture icon; fails without the `link_imports` change); the apt_text
  wrap test; `cargo test -p skate-mods` menu commands; `python -m unittest tools.asset_pipeline.test_menu_tables`.
- In game (dev build, tables extracted into the install for the shots, then restored): Learn > Trick Guide opens; the
  categories, Flip Tricks > Ollie Flips, Kickflip with its gesture and the wrapped description, Other Tricks > No Comply
  with A, Arrow, stick, A / B prompts. Pause menu and Game Settings unchanged.
- Known unrelated failures: skate-core `a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`
  (doc 11), skate-mods `skyline_every_component_is_real_and_drives_through_ground_contact` (Skyline GLB not in a
  worktree).

### Open questions
- The 3D demo behind the screen: not ported; the world shows instead (FE 14 has no blur). A local research
  note (not verified yet): init loads
  `world/models/DIST_TrickGuide_Textures.rx2` + `DIST_TrickGuide.rx2` (miscload.big, a backdrop set) and
  `rendering/csm_trickguide` unless `sub_82743FC0` (online state 11); close vt+24 0x82618CA8 unloads; the update posts
  message 0xF00F5033 (trickbook mode) and 0x58B0BD5C {clip, camera track} to the trickbook object (handlers
  `sub_82821810`), which plays `scene/trickguide/<name>1.abin` (scene.big, 312 clips, not in our extract) through the
  cutscene Scene system ("ViewScene"); `trickbook_camera_tracks` = Int16 index (22 records).
- The fade before open / close: ported, see "Fade to black".
- Remap controls byte (`+224/+20`) and the X setting byte (`+260/+37`) not decoded (defaults: off, X reads A).
- Font+360 is LineHeight or VAdvanceYMax (equal in every retail font); the wrap flag is taken as `word_wrap`.
- `SetSkin` on the menu picker is unhandled (the menu_picker class has none; likely a no-op in retail too).
- Ties in the trick order follow record order; retail's intro sort could order ties differently in lists over 28.

## Fade to black (2026-10-12)

### Problem
Opening and closing the Trick Guide switched screens at once; retail fades to black first and back in after.

### Root cause / evidence (TU3, verified)
- The FE fader `[[0x830CFE14]+304]`: +12 alpha (1 = black), +16 step, +20 hold bits, +24 actions waiting for black.
  Hold `sub_825E6B30(fader, bit, callback, instant)`: sets the bit; already black = run the callback now; else queue
  it and step 0.04 (0x82216FEC; instant 1.0). Tick `sub_825E69A0`: alpha += step, clamped; at 1 the queued callbacks
  run. Release `sub_825E6BF8(fader, bit, instant)`: clears the bit; with none left and the screen not clear, step
  -0.04 (0x822F88F8; instant -1).
- Trick Guide: the crossbar case 0x8261BF78 holds bit 0 with callback 0x8261CDD8 (push FE 14) and sets its closing
  flags; screen 14's update `sub_82619A20` state 4 counts 11 updates (S+336 > 10), then releases bit 0 and goes to
  state 2, the only state its input handler takes keys in. Back at level 0 holds bit 0 with callback 0x82618EF0
  (close). The crossbar's init (vtable 0x82303F90 slot 3, `sub_8261ACA8`) releases bit 0, so the pause menu that comes
  back fades in.

### Change
- `skate-core/src/fe_fade.rs` (new): the fader (hold / release / tick, actions at black), `FeFadeParams` (steps and
  tick rate as data, retail's the defaults), FE cadence like fe_blur.
- `retail_menu_stage.rs`: TrickGuide select holds the fade and pushes FE 14 at black; the guide counts 11 ticks after
  its intro and releases; its Back at level 0 holds and closes at black; the pause menu's open releases; no pause input
  while the fade is held, no guide input before the release; a black quad over the menu layer with the fader's alpha;
  `RETAIL_MENUS fade black / clear` log lines.
- The FE tick is 60 a second (measured 2026-10-24, "FE fade rate" below; the blur's 30 stays assumed). NOT RETAIL YET: what enters screen 14's state 4 (the count starts at the
  intro's end); Game Settings still opens without the fade (its crossbar case not checked). NOT RETAIL (guard): with
  no screen left on the stack the fade is released (retail always returns to the pause menu).

### Verification
- `cargo test -p skate-core fe_fade` (25 ticks to black, the action at black, release order, instant, cadence).
- In game (tables extracted for the run, then restored): select Trick Guide: `fade black`, the pause menu closes, the
  guide opens, `fade clear` after the count; B at level 0: `fade black`, the guide closes, the pause menu opens again,
  `fade clear`.

## Trick Guide stance swap (2026-10-29, research b672)

**Problem.** The Trick Guide's regular / goofy button remap (`sub_82619FC0`) applies when exactly one of the natural
stance (regular) and TrickDisplayManager+20 is set; the second was never fed (always false).

**Retail [code, b672].** TrickDisplayManager+20 is synced each tick by `825E4D68` (only caller `825E4F40`, from the FE
manager update `825D8B50` in game state 5) from `[0x83067070]+40`, which only the per-tick player stats tick
`82775328` writes: byte +0x9D of the skater's physics output block, copied from animation packet +10371 = SkaterAnim
vt+0x18 `82B97128` (`+0xEC == 1`): the skater is riding switch. So the remap follows the effective stance (natural
XOR switch), not a setting.

**Change.** `retail_menu_stage.rs` sets `Stance.alternate` from `SkaterAnimation::riding_switch` next to `regular`;
the `Stance` doc comment says what it is. **Verification:** skate-game trick guide tests pass. Not seen in game.
**Open:** a mod override of the layout.

## Trick Guide demo (2026-10-12)

### Problem
Retail shows a 3D demo behind the Trick Guide (FE 14): the player's skater performs the highlighted trick in a small
set. Ours showed the gameplay world behind the menu.

### Root cause / evidence (TU3; research notes on the Trick Guide and the raw codec)
- [code, verified] Screen init 0x82618A40 loads `DIST_TrickGuide_Textures.rx2` + `DIST_TrickGuide.rx2`
  (miscload.big) unless online (`sub_82743FC0`), copies the default clip `trickguide_fs_noseblunt_01` (0x82204B88)
  and attaches `rendering/csm_trickguide`; the clip path is built by `sub_82826488` as
  `"scene/trickguide/" + name + "1.abin"` (0x82198B78); `sub_82821740` stores the camera track and starts the clip.
- [data, verified] scene_000.bin (miscboot.big): `ViewScene` = record 254 (type 22, 5.0 s), one character, name
  `current_player`, kind `marquee`, default clip `trickguide_bindpose`. So the demo shows the player's own skater.
- [research, high] The highlight settles 60 ms (S+332 = 60.0 counted down by the FE update's milliseconds) before the
  clip changes; the scene restarts at time 0 only on a new clip.
- [code, verified anchors] The clips (scene.big, 312 files) use the RAW codec "DR". Its TU3 sampler
  `sub_82D1E658` reads the header bytes +27 / +28 and the frame size +24; packed rotations are an axis in two angles
  (A = bits 21..31 x 2pi/2047 - pi, M = bits 11..20 x pi/1023 - pi/2) times r = bits 0..10 / 2047, with
  w = sqrt(1 - r^2) (pi at 0x82060C44). The pinned community decoder (tools/vendor/skate3_anim) read the middle field
  as a length, which folded the skeleton; with the sampler's formula the file's T-pose decodes to the gameplay
  `RIG_TPOSE` to three decimals.
- [data, NOT RETAIL YET] The clip channels are deltas on the file's pose (limbs near identity where the pose has large
  rotations; the HIPS channel is the inverse of the bind trajectory). Each frame = retail's AddSQT (`pose_add::add`,
  0x828CCC08) of the delta onto the pose, root bones hang from the trajectory. Checked: the bind pose stands (hips
  0.92 m, head 1.58 m, feet 0.11 m, mirrored); 302 of 312 clips start and end upright, the other 10 are Hall of Meat
  bails that end on the ground.
- [data] One authored NaN (trickguide_bs_dark_slide1 frame 103, RIGHT_WHEELBACK translation), also in the
  reference decoder: the previous frame is held.

### Change
- `skate-core/src/animation/raw.rs` (new): the RAW decoder (stored components over a caller's base).
- `skate-data/src/scene_clip.rs` (new): `SceneClip::parse` (131-bone hierarchy, parts by header bits 0..5 / 2,
  deltas added onto the file's pose).
- Setup: `tools/asset_pipeline/scene_clips.py` + task `tools/prepare_trick_guide_scene.py` (hud group): the clips to
  `assets/private/hud/scene/trickguide/`, the set as a render-only `TrickGuide.skate` (`export_set`, the format of
  backdrop.py's global-model export, kept apart from backdrop.py so this branch and the living-world branch merge
  cleanly), and the set's sky dome as
  `TrickGuide.sky.*` with its `material_sky` parameters.
- `skate-game/src/trick_guide_scene.rs` (new): while FE 14 is on top the set and dome spawn on render layer 27 as
  children of the player root, the player's own rig plays the clip (`AnimationStatus::for_scene`), the meshes the
  gameplay camera draws join that layer (hidden rigs stay out: a hidden default skater under a custom model showed
  through on hails-additions), the gameplay camera draws only that layer; closing despawns them and restores layers. `retail_menu_stage.rs` publishes
  `TrickGuideDemo` (open, highlighted clip); `map_render.rs` `SceneCommands::apply_now` (the demo re-tags what it
  spawned: its own marker instead of `MapEntity`, parented to the player root, on the demo layer); `retail_sky.rs`
  `SkyPackage::load_scene`.
- Mods: a mod picks any clip per entry through the existing Trick Guide patches (`sdk.menus.trick_guide`, field
  `animation`). Mod clip folders are not wired yet.
- NOT RETAIL YET: looping (assumed), the delta add and trajectory parenting (from the data). The camera, fog and
  sun: see "Trick Guide camera, fog and sun" below.

### Verification
- `cargo test -p skate-core animation::raw`; ignored data tests `crates/skate-data/tests/scene_clip.rs` (all 312
  decode, T-pose stands, upright ends) and `tests/trick_guide_checklist.rs` (217 trick rows, all with a clip); script
  `tools/claude-skills/retail-menus/tools/check_trick_clips.sh`.
- In game (default skater and a custom model): the set, the dome and the skater playing the noseblunt
  (local screenshots); gameplay after the change unchanged (Downtown screenshot).
  Closing the guide in a run was not checked (injected keys arrive before the guide takes input); play test pending.

### Open questions
- Whether the scene loops the clip (`TrickGuideLoop` = enum 9). Camera and environment: next section.
- 61 clip files and 9 clip names no listed row uses (8 tricks with no subcategory, the unlisted "Retrieve"
  subcategory): retail does not show them either.

## Trick Guide camera, fog and sun (2026-10-12)

### Problem
The demo used a fixed side view of the hips, no fog, and the current map's sun on the set's dome.

### Root cause / evidence (TU3 recomp code and the disc data; research notes on the Trick Guide camera, fog and shadow map fields)
- Each Trick Guide entry names a camera track: RefSpec 0x3D3A79F16DCB54BB (a ClassRefSpec: record key in the first
  8 bytes) into `trickbook_camera_tracks`, whose Int16 is the track index; no ref reads 0 (`sub_826188B8`, S+200);
  the screen init sets track 1 with the default clip. [code + data] The installed data uses tracks 0, 1, 3, 18, 19, 20.
- The tracks are the ICE ViewScene block (scene hash 0x7F298808) in scene.big `data/scene/ice_000.bin`: 21 records
  (`sub_828F51E8`), each with 37 key groups; the camera elements (EYE, LOOK, DUTCH, TANGENT_EYE / TANGENT_LOOK,
  LENS_LENGTH) sit in group 0 (element table `sub_82903840`; EYE's descriptor names TANGENT_EYE as its tangent
  element, +64 = 7, and element 87 at +60). [code + data]
- Interpolation (read in the code): `sub_828F0BB0` sets each element up per segment as a cubic Hermite
  `((a*u + b)*u + c)*u + p0`, a = m0 + m1 - 2D, b = 3D - m1 - 2*m0; the tangent `sub_828F0990` is the non-uniform
  Catmull-Rom tangent in the segment's own parameter (neighbour segment weighted by the durations; none at the ends:
  D), scaled by the tangent element's value at that key. [code]
- FOV: `sub_828F1E80`, `2*atan(15.96 / lens_mm)`, lens clamped to 10..500 mm. [code]
- While the set is loaded, the renderer's environment is render location `trick_guide` (`sub_827950A8`: model list
  +17496 == 1 selects key 0x6568F34643066684, verified): parents `fe`, `default`; sun (0, 0.8, 0.1), anchor 165,
  `fog_trickguide` (fog_default made black: near 10, far 200, power 1.9, max 0.8). [code + data]
- `csm_trickguide` differs from `cascade_shadowmap` in the per-cascade shader bias `g_CSMSelfBias` (0.005, 0.001,
  0.024 vs 0.0025, 0.005, 0.012), a Bool and a pair (-2, 4) stored next to each other with no reader found; both
  records share the cascade splits 1.8 / 7.2 / 28.8 m. [code + data, research; the bias name by its hash]

### Change
- `skate-core/src/camera_track.rs` (new): `CameraTrack::sample` (the Hermite and tangent above), `fov_radians`.
- `skate-data/src/ice_camera.rs` (new): `scene_tracks(bytes, VIEW_SCENE)` reads the block (little-endian; key times
  u16/65535 of the length, quantised elements MSB-first bit streams).
- `skate-data/src/trick_guide.rs`: `Entry.camera_track` from the ref; `skate-core` `TrickGuideState.camera_track`
  (S+200) and `EntryPatch.camera_track`; mods: `sdk.menus.trick_guide` option `camera_track` (track index).
- `trick_guide_scene.rs`: the playing clip's track is fitted to the clip length (retail "Fixing Camera Group
  length"), placed on the skater with the height zeroed, and drives the gameplay camera's transform, roll and FOV
  (restored on close); track -1 or no `ice_000.bin` keeps the side view.
- Setup `scene_clips.py`: exports `ice_000.bin` next to the clips, and the set's sky package carries the `trick_guide`
  location's sun, anchor height and fog frame (`scene_environment`); `retail_sky.rs` `load_scene` uses it (older
  exports: the map's sun, no fog), and the set's materials take its fog.
- NOT RETAIL YET: the locator and axis map (inferred: the tracks aim at a skater near the origin while clips travel
  up to 70 m, so the track follows the skater with height zeroed, "TrickbookPlayerAtZeroY"; track +y = the clips'
  run direction, Z up: scene = (y, z, x)), the lens angle as the vertical FOV, DUTCH as radians and linear, LENS with
  tangent 1, the spline switch (element 87) assumed on, key times and dequantisation (from the data), the record walk
  (by the constant +4 word: the element table past id 17 is not decoded). The location's exposure and post_fx refs
  stay the map's. `csm_trickguide` is not applied: our shadows do not port the base `cascade_shadowmap` record yet,
  and its own differences are a per-cascade bias Bevy has no slot for and fields with no known reader.

### Verification
- `cargo test -p skate-core camera_track` (key hits, linear two-key case, ease with tangent 0, Catmull-Rom on
  collinear keys, the 24 mm angle); ignored data tests `skate-data --test ice_camera` (`SKATE3_ICE`: 21 tracks,
  lengths, track 0 values, track 3's interior key, finite samples) and `--test trick_guide` (track indices 0..20 in
  use, 3 and 18 present).
- In game with the dev export: the default clip with track 1 frames the skater left of the menu, facing the camera;
  the fog is applied (faint at the set's distances, as its values give). Other tracks are not screenshot (injected
  keys arrive before the guide takes input). No retail picture of the demo to compare yet.

### Open questions
- Settled by the recomp trace (next section) except horizontal vs vertical FOV: the locator, the axis map, DUTCH
  units, element 87 and the track time fitting.
- Who reads the csm Bool / pair; mapping `cascade_shadowmap` + `csm_trickguide` onto our cascades.

## Trick Guide camera from the recomp trace (2026-10-12)

### Problem
The camera port above guessed the parts the code reading had not settled: where each track sits (every track was
placed on the skater with the height zeroed), the axis map, DUTCH as radians and linear, LENS with tangent 1, the
spline switch always on, the FOV units and the track time fitting. Tracks 3 and 19 framed the wrong view.

### Root cause / evidence (TU3 recomp code, guarded trace hooks in the recomp and the disc data; research notes on the Trick Guide hooks and the ICE space elements)
Three muted background runs of the recomp opened the guide and highlighted rows covering tracks 0, 1, 3, 18 (Jump),
19 and 20, logging the per-frame camera matrix, the element values, the clip fit and the camera player's element
words (64k frames, no malformed lines). A watch-list run read the element descriptors live.
- Element values live at player + 16 + 4 * id (`sub_828EEA20`); descriptor = `[0x830BBEA8 + 4 * id]`. [code, main
  checked]
- Spline switch and tangent per element (descriptor +60 / +64, read live): EYE_X..Z, DUTCH and LENS_LENGTH use
  CUBIC_EYE (87) / TANGENT_EYE (7); LOOK_X..Z use CUBIC_LOOK (88) / TANGENT_LOOK (8); 65535 = linear.
  `sub_828F0990` reads the switch element's value; 0 gives the plain segment slope. [code + trace]
- CUBIC_EYE, CUBIC_LOOK, SPACE_EYE / SPACE_LOOK (89 / 90) and AVATAR_EYE / AVATAR_LOOK (91 / 92) are one value per
  segment of key group 0 in each track record. The record layout needs the whole element table: every element in id
  order takes `((bits * count + 31) >> 3) & ~3` bytes, count = segments or keys of its group (`sub_828EE070`). The
  table (148 elements: name, group, per-segment flag, bits) comes from a static emulation of `sub_82903840`;
  ids 0..12 and their links were checked live, and the record sizes chain all 21 tracks exactly. [code + data]
- Placement (`sub_828F2C38` -> `sub_828F36B0`): SPACE 2 adds the position of object AVATAR - 1 from the ICE game
  interface `[0x830840F4]` (vt+20 = `sub_82673560`) without rotation; other SPACE values with no case leave the point
  fixed in the set. AVATAR 5 and 7 both read the Trick Guide scene instance (`[[[0x830CFE44]+4]+40]` vt+120 =
  `sub_828278A8`, key = the scene key 0x00010001 while the guide is open): the translation of the first clip entry's
  transform, i.e. the demo skater's root; AVATAR 5 zeroes its height, 7 keeps it. The getter returns game positions as
  (z, x, y) in track space, so the scene maps a track point (x, y, z) to (y, z, x). [code + trace]
- Per track (trace and disc agree): tracks 0, 1, 18, 20 SPACE 2 / AVATAR 5 for eye and look (the camera moves with the
  skater, height never changes, up to 85 m); track 3 eye SPACE 1 (fixed), look SPACE 2 / AVATAR 7 (aims at the
  skater's root at full height); track 19 SPACE 1 for both (fixed). CUBIC_EYE and CUBIC_LOOK are 1 on these tracks.
  [trace + data]
- DUTCH: `sub_828F2C38` multiplies it by 65536 into a 16-bit angle, so it is in turns; the traced matrix rolls by
  -2 pi * DUTCH about its forward axis. [code + trace]
- FOV: the stored angle equals 2 * atan(15.96 / lens) degrees within 0.012 degrees on every frame. [trace]
- Track time: `sub_82820E98` writes the clip length into the track's length (+12) for every new clip (track 0 took
  4.43, 4.62, 2.38, 7.25 s for successive tricks), so the track runs in clip seconds; our fit gives the same key
  positions. [code + trace]
- The camera matrix leaves `sub_828F2C38` unchanged through `sub_828F2B58` and on to the camera (shake is off in the
  guide). [trace]

### Change
- `skate-data/src/ice_camera.rs`: `ELEMENTS` (the element table), per-element offsets, the per-segment settings of
  group 0, and the record walk by size (replaces the search for the next record's constant word).
- `skate-core/src/camera_track.rs`: `CameraSegment` (CUBIC_EYE / CUBIC_LOOK, eye and look `CameraSpace`), per-segment
  switch (off = linear), DUTCH and LENS splined with CUBIC_EYE / TANGENT_EYE; `CameraSample` carries the spaces.
- `trick_guide_scene.rs`: `place` puts each point by its segment's space (SPACE 2 + AVATAR 5: the clip's root at
  height 0; AVATAR 7: the full root; other SPACE values: fixed in the set); DUTCH applied in turns.
- `retail_menu_stage.rs`: `_` / `_xN` in `SKATE3_RETAIL_MENUS_KEYS` inject empty frames (dev screenshots; the guide
  opens after its fade, so later keys need a pause).
- Moddable: the segment settings are part of `CameraTrack`, so a mod that replaces or adds a track by index sets them
  too; the element table is the file format (stable retail ids and names).

### Verification
- `cargo test -p skate-core camera_track` (5: key hits and the linear two-key case, ease with tangent 0 and the per-segment
  switch off, Catmull-Rom on collinear keys, DUTCH and LENS following the eye's tangent, the 24 mm angle).
- `cargo test -p skate-data --test ice_camera -- --ignored` (`SKATE3_ICE`): the size walk finds all 21 tracks with
  their lengths, and tracks 0, 1, 3, 18, 19, 20 decode to the settings the recomp's camera player held.
- In game (dev export): Skitching (track 19) frames the same view down the set as the recomp at that row; Pumping
  (track 3) holds a fixed camera that aims at the skater (not a frame-matched comparison: different clip times).

### Open questions
- Horizontal or vertical FOV (the projection reader of the stored angle is not found).
- Which point of the clip vt+40 returns (bone 0 trajectory vs another root; the measured drift fits bone 0).
- SPACE 0 (player locator with orientation) and 3 (an object's full matrix) and the other AVATAR slots: not used by
  the Trick Guide tracks, they follow the skater at height 0 and log once.
- The other per-segment flags (AVATAR_ORBIT, AVOID_*, PLAYER_SPACE_*) are not read yet. PLAYER_SPACE_LAG (98, set on
  tracks 0 and 3) subtracts an offset from `sub_828F3088` from eye and look; the trace did not separate it from the
  follow offset.

## Settings without an effect are greyed (2026-10-13)

The user's decision: settings that do not work yet are greyed out, like the other retail menu items.

### Change
- `retail_menus.rs` `NO_EFFECT_YET`: Subtitles, Minimap, Cameraman Mode, HOM mode, Units, Transparency and the four
  skate.Feed toggles get no stored value layer, so they stay greyed and show retail's default; values saved by
  earlier builds stay in `settings/game-settings.json` untouched. A mod's value layer (`menu_handle` with a value)
  still enables a row. An id leaves the list when its feature lands.
- Rows that work (step, apply, persist): Play Mode, SFX / Dialog / Music volume, Camera Angle (engine bindings),
  HUD, Offboard Y axis, Vibration (stored rows), Video Calibration (opens its screen).

### Verification
- `cargo test -p skate-game rows_without_an_effect_stay_greyed_until_a_mod_binds_them`.
- A local screenshot: Video Settings with Subtitles, Minimap and Cameraman
  Mode greyed; Camera Angle, Video Calibration and HUD enabled.

## Video Calibration (2026-10-13)

### Problem
Game Settings > Video > Video Calibration was greyed (no handler).

### Root cause / evidence (TU3 recomp code, every handler named here re-read; research note on video calibration)
- Row 25 in the settings select handler `sub_8260D290` plays core_a_button and pushes FE screen 51 through
  `sub_82D0AEF0(FE, 51, ..)` with param 0, keep-open 1, keep-visible 0 (Game Settings stays open, hidden). FE 51's info
  record (key `1f3e2788f93b6a2e`) suspends gameplay, shows the chyron, HUD off, no blur, unused keys to the movie.
- Screen 51 (vtable 0x82303548): init 0x8260E868 sets the page counter +60 to 0 and opens template 91
  VideoCalibration (`options/videocal.swf`); loaded 0x8260E888 and A run 0x8260E950: below 3 it posts SetState(counter)
  and counts up, else it pushes the "finished" popup (ID_VIDEO_CALIBRATION_FINISHED_TITLE / _DESC, Yes / No, callback
  0x8260EA48: choice 0 pops the screen, any other restarts at page 0). Input 0x8260E890: A (core_a_button) advances,
  B (core_b_button) pops below counter 2, else SetState(counter - 2) and counts down; it always returns 0.
- The movie calls no Game.* native: pages settings / contrast / brightness (SD variants when Screen_EdgeOffset > 0);
  nothing is changed or saved by the game. The A / B prompts come from button_menu (`AddItem`), coloured 0xFFFEEC
  through its SetInfo -> `setTextFormat`.
- Hidden / shown screens: the facade's ShowScreen (0x825DA410) passes the AS id as "%d" and "true" / "false".

### Change
- `tools/prepare_menu_movies.py`: root `source/screens/options/videocal` (setup's hud group re-exports; the row stays
  greyed in sets without it).
- `skate_core::video_calibration` (new): the counter as retail (`advance`, `back`, `popup_result`), rules as data
  (`CalibrationRules`: page count, popup strings, sounds; retail's the defaults).
- `retail_menu_stage.rs`: FE 51 opens / closes its template; A / B as 0x8260E890; loaded posts the first page; settings
  rows whose engine handler names an FE screen push it with retail's flags; `Effect::Show` reaches the movies through
  `_global.ScreenManager.ShowScreen`. The row's engine handler (`Owner::Engine("screen:1f3e2788f93b6a2e")`) is
  registered when the movie is in the set; a mod can register its own handler on the row.
- `apt_stage.rs`: `TextField.setTextFormat` applies the format's colour as the field's text colour. NOT RETAIL YET:
  standard AS2 semantics, whole field, font and size not applied.
- The A / B prompts are the native CustomButton glyphs (doc "Button prompts"), so they follow the connected pad.
- The "finished" popup is retail's (see "Popups" below): Yes closes the screen (Game Settings shows again on the
  Video screen), No starts over at the first page; B does nothing (not cancelable, as retail).

### Verification
- `cargo test -p skate-core video_calibration` (3: retail paging, popup Yes / No, page count as data).
- `cargo test -p skate-game retail_video_calibration_steps_its_pages` (data-gated on the menu set): the retail movie
  loads, reports loaded / intro complete and takes SetState 0..2 with no Game.* call unanswered.
- In game (dev export, `SKATE3_RETAIL_MENUS_KEYS` through Options > Game Settings > Video > Video Calibration):
  a local screenshot (the settings page with colour bars, A Continue / B Back),
  `page1.png` (Contrast after A).

- In game: a local screenshot (the popup over the dimmed brightness page), `yes.png`
  (back on Video Settings), `no.png` (the first page again).

### Open questions
- Does retail pick the SD pages on a 720p console? No: on HD retail never sends the "136" edge offset, so the
  offset stays 0 and the movie shows the HD pages (answered 2026-10-24 by the loading screen, "Loading screen (FE
  52)"); our boot no longer sends it either.

## Popups (2026-10-13)

### Problem
Retail asks questions through its popup system (Video Calibration's "Are you happy with your video calibration
settings?"); the engine had none, so the calibration screen auto-answered Yes.

### Root cause / evidence (TU3 recomp code and the menu movies; research note on the FE popup; re-read the host's show 0x825EF440, choice 0x825EF698, create 0x825EF370 and the OpenScreen bridge 0x82D09140)
- A caller fills an sPopupInfo (1452 bytes, defaults `sub_82D0B0B0`: title, description, up to 8 options, choice
  callback, user value, type, priority, default selection, cancelable, auto-remove, keep-on-clear) and pushes it with
  `sub_82D0A8D0`: it gets the next id and joins a list sorted by priority (ascending, ties in arrival order); the
  last entry shows (FE vt+24 `sub_825EC530`).
- The popup is not a stack screen: FE screen 29 is a fixed host. Its create (0x825EF370) opens templates 17 Sk8Popup,
  18 Sk8SmallPopup and 4 Sk8AutoSavePopup once through the OpenScreen bridge with layer "popup" (the template's visible
  flag "0"); the screen manager puts them under `gPopups`. Show (0x825EF440): hide all three (ShowScreen false), pick
  by type, SetScreenState(1), ShowScreen(true), sound core_popup, input ready.
- The movies pull their text through the AS `Popup` natives (registrar `sub_825A9F40`: GetTitle, GetDescription,
  GetNumOptions, GetOption, IsPopupCancelable, GetDefaultSelection). While the list has entries every key goes to the
  host only, its C++ handler returns 0, so the movie's UpdateInput takes it: Up / Down move menu_picker, A calls
  `Game.OnMenuSelect(menu, index, text)`, B calls `OnMenuSelect(0, 255, 'CANCEL!')` only when cancelable.
  `Game.OnMenuSelect` goes to the focus screen (`sub_82D09F28`: the host while a popup is up); the host calls
  `callback(id, choice, user)` and, with auto-remove, removes it (`sub_82D0A628`) and shows the next.
- Sk8Popup shows the description and the options (no title); the small popup shows the title.

### Popup kinds
(2026-10-25, USER-ASKS 351 "tried "enter emerica shoe code" and got stuck.")
- **Retail [code / data]:** the sPopupInfo constructor `sub_82D0B0B0` sets type +1436 = 0 (Sk8Popup), option count
  +384 = 0, auto-remove +1449 = 1. The cheat matcher `sub_82656080` fills title +0, description +128, one option
  (+384 = 1, text +388) and pushes it with `sub_82D0A8D0` without touching +1436, so every cheat dialog is a
  Sk8Popup with its OK / Continue option.
- **small_popup [data, movie script]:** `Refresh` reads only `GetTitle`, `GetDescription` and `IsPopupCancelable`
  (frame "nobutton" when not cancelable, "button" with a fixed B ID_COMMON_CANCEL help when cancelable); it never
  reads the options. `UpdateInput` answers only AptPrevious with `OnMenuSelect(.., 'CANCEL!')`. A small popup is a
  notice that code removes, or B closes when cancelable.
- **Bug:** ours pushed the cheat dialogs, the Extras code-redeem popup (ours), the call-skater limit popup and the
  keyboard warning (record 0x107) as small popups with an option and not cancelable: the movie drew a bottom banner
  without a button and no press could close it, so the screen below went dead.
- **Change:** all four use the default kind (Sk8Popup). `show_popup` warns when a small popup has options but is not
  cancelable (a mod's `sdk.menus.popup` with `kind = small` can still ask for one).
- **Limit popup and record 0x107 [code + data, research b607, verified the limit push]:** both Sk8Popup. The limit
  popup (`sub_8264DB30`) sets +384 = 1 and callback +1412 = 0 and never writes +1436. FE popup table records (base
  0x83028050, stride 0x38, pusher `sub_825DECD8`): +0 kind, +4 title, +8 description, +12 cancelable byte, +16 option
  count, +20 default selection, +24 options; record 0x107 = kind 0, not cancelable, one option. Only the table gives
  kinds 1 to 3 (kind 1: the autosave warnings; kind 2 / 3: online notices, the media and keyboard warnings, 0xC8
  "generating footage").
- **Kind -> movie and closing [code, research b608; matches `PopupKind::from_retail`]:** the host show `0x825EF440`
  maps 2 / 3 to Sk8SmallPopup, 1 to Sk8AutoSavePopup, anything else to Sk8Popup. Of the 139 direct pushes only three
  records have a non-zero kind: 0x17 (autosave), 0x69 ONLINE_LATENCY (kind 3, removed by its owner with
  `sub_82D0A628`) and 0xC8 GENERATING_FOOTAGE (kind 3, cancelable, no options). The kind 2 records with one option and
  no cancel (0x93 to 0x96, 0xFC, 0x102 to 0x105) are never pushed in TU3, so retail never shows a small popup that
  only an option could close; ours now matches.
- **Files:** `crates/skate-game/src/retail_menu_stage.rs`.

### Change
- `skate_core::fe_popup` (new): `PopupRequest` (sPopupInfo as data, retail defaults), `PopupList` (retail ordering,
  ids from 0, choose with auto-remove, remove, clear-all with choice -2, a mod's popups by callback prefix).
  Callbacks are stable string ids, so requests and events are plain, serialisable data.
- `popup_host.rs` (new): the `Popup` natives from the shown request; while a popup is shown it answers
  `Game.OnMenuSelect` / `OnMenuGetItemEnabled` (every item enabled, host vt+72) / `OnMenuHilight`, so the choice does
  not reach the screen below.
- `retail_menu_stage.rs`: the popup movies open on the "popup" layer at boot (those in the set); popup show as
  0x825EF440; keys go to the shown popup first; choices run their callback (Video Calibration's
  `video_calibration.finished` -> `popup_result`).
- Mods: `sdk.menus.popup(key, options)` raises or replaces the mod's popup (description, title, up to 8 options, kind
  standard / small, priority, cancelable); the choice arrives as `on_event {name='menu_popup', key, choice}`; nil
  options take it down; a stopped mod's popups leave without an event.
- APT player features the popup movie needed (standard AS2 semantics, each NOT RETAIL YET in code):
  `MovieClip.createTextField` (a script-made text field with its own descriptor), `setTextFormat` /
  `setNewTextFormat` font / size / alignment on such fields (font by name, else FontManager's fallback row, exported
  as `fallback_font` with its font), `wordWrap` on script fields, `TextField.maxscroll` (the wrapped line count: the
  popup sizes its panel with `body.maxscroll * bodyLineHeight`), arrays growing `length` on an indexed write
  (menu_picker counts its items with `mItemTextArray.length`), and the stage drawing loaded movies by their holder
  clip's place in the display tree (`display_path`: popups under gPopups draw over the screens) instead of load order.

### Verification
- `cargo test -p skate-core fe_popup` (4: priority and tie order, auto-remove and clear-all, retail defaults and the
  8-option cap, a mod's popups leave with it); `cargo test -p skate-game popup` (natives and focus routing, mod
  popups); `cargo test -p skate-mods` (the `menu_popup` command parses and validates);
  `retail_popup_builds_from_the_popup_natives` (data-gated: the retail Sk8Popup builds its panel from the natives
  with every layout value finite, and A answers 0).
- In game: the calibration popup, Yes and No (Video Calibration "Verification"); the pause menu screenshot is
  unchanged with the new draw order (a local screenshot).

### Open questions
- Host +80 ("suppressed"), sPopupInfo +1450, the per-update / timeout callbacks (+1416 / +1420 / +1424, not ported:
  no ported popup uses them), the 8th Popup native, the AutoSave popup movie (template 4) is not in the export.
- APT's real `maxscroll` and text format rules (only what the retail movies need is implemented).

## Cameraman Mode (2026-10-13, decoded, not ported)

Local research notes (research; verified the S+165 branch in
`sub_827F4D28`, the rig hashes and the +0x34788 flag in a recomp watch run).
- [code] The setting is a look switch, not camera behaviour: `sub_82801990` sets the scene render manager's flag
  `[0x83083C5C]+0x34788` from S+165 (forced on in cutscenes, off in the skatepark state +0x34786); with it set,
  character presentation objects pick LightRig "nis" (FNV-1 0x1E7BA14D) instead of "default" (`sub_827823D8` ->
  `sub_827A5A78`), draw an extra character pass (`sub_82784298`), and `sub_827F4D28` sets post mode 4414 (S+165 on)
  instead of 21 / 286, which `sub_827EE180` turns into technique 0x20559385 instead of 0xBFBCDB6B.
- [trace, the recomp, 2026-10-13] Gameplay flag bytes +0x34784..+0x34788 = 00 01 00 01 01 with it on; switching it
  off clears +0x34788 at once.
- [code] A rig is a shader variant per material binder; its light values come from run-time globals, so a port needs
  a short recomp hook logging the binder constants (`sub_82CBBBF0` nis / `sub_82CBD050` default) with the setting on
  and off. Our characters use one fixed rig (`retail_character.rs`). Greyed until then.

## Loading screen (FE 52)

(2026-10-24, USER-ASKS 304: "begin research and porting the retail loading screen to be used as it would hide all
loading shenanigans in the background". Research: the local front-end menus note, "Loading screen" (2026-10-08 static
pass and recomp timing run), research b581 (port plan); claims below verified.)

- **Retail [code / trace]:** loads run behind the tips loader, FE screen 52 (template 131 TipsLoader, movie
  `screens/main/tiploader`, screen record key hash64("tiploader") `e1fa207da2bdf61a`, disc index 50). Order from the
  recomp trace: the fader goes black (0.04 per FE update), the screen is pushed at black, the black goes at once when
  it is up, one panel shows (9.0 s each, 0.5 s outro, then the next pick), and when the load is done the fader goes
  black again, the screen closes, about 4 updates of black, then the fade back in. The panel pick (sub_82608598):
  least shown first, then shown more, then shown before, then the one shown last; uniform within the group. Coach
  Frank's line and the hint (sub_82605FA8 / sub_82606C98): uniform over every line but the previous one. Offline
  panels: Coach Frank, Hint, and Stats when there are stats rows; Leaderboard, Feed and Reel need online data.
- **Ours:**
  - Setup exports the movie with the menu set (`tools/prepare_menu_movies.py` MENU_ROOTS; optional like the action
    overlay, so the set version stays 14 and a set without it keeps the PC overlay's progress text). The vault
    converter decodes the hints' (title, text) string pointers (`vlt.py`, `text_pairs`); the rest of the vault JSON is
    byte-identical (checked against the installed one). Both need Refresh Game Files.
  - `skate_core::tips_loader`: the panel pick, the line pick, the panel cycle (`Timing::RETAIL`) and the load
    sequence (`LoadScreen`, steps HoldBlack / Push / ReleaseInstant / Close / Release, `BLACK_FRAMES` 4).
  - `skate_data::tips_loader::read`: the `tips_loader/default` lines (20 Coach Frank ids in the record's order, 31
    hint pairs).
  - `tips_loader_host.rs`: the `TipsLoader` natives (`Is<X>Enabled` = the picked panel, `GetCoachFrankText`, `GetHint`,
    `GetStats(n)`, empty online answers). Stats rows: none yet (the engine has no stats table, see Skate Profile), so
    the Stats panel stays off, as retail with no stats.
  - `retail_menu_stage.rs`: while a map transition runs the stage runs the load
    sequence on fader bit 2 (sub_826E08A8's bit), pushes FE 52 at black, answers its intro, posts the panel states and
    draws black under the movie while it covers. Keys are ignored while a load runs. `map_transition` waits with the
    commit until the loading screen covers the screen (`LoadingCover`, at most 5 s), so the old world never changes in
    view; the map's slice upload (doc 25) runs behind it.
- **The movie in our VM:** SetState(1) asks every panel whether it is the picked one and fills that one
  (`tiploader::UpdateCoachFrank( true true )` with `GetCoachFrankText`); SetState(2) runs the outro. The disassembly's
  empty Populate branch (b581) was a disassembler gap.
- **Tests:** `skate_core::tips_loader::tests` (5: line pick, panel buckets, Stats joining, cycle, load sequence);
  `skate_data::tips_loader::tests::the_retail_tips` (data-gated); `apt_stage::tests::retail_loading_screen_shows_a_tip`
  (data-gated: a set with the movie); the map transition tests.
- **FE fade rate: 60 updates a second** (2026-10-24, USER-ASKS 326). The user's retail video (PS3 game in RPCS3,
  clip of 2026-10-21 08:45, 60 fps) fades back in after a Challenge Map teleport's load in 25
  frames (19.38 to 19.80 s), the brightness falling by 0.04 of full (3.75 of 93.8) on every 1/60 s frame [video]; the
  recomp's `LSRATE` counts 303 fade updates in 5 s with a 16 ms step [trace]. `FeFadeParams::RETAIL.tick_hz` is now 60
  (a fade 0.42 s instead of 0.83 s); the load sequence, its 4 black updates and the Trick Guide's state-4 count run on
  the same ticks. The fade runs in the FE update `sub_825D8B50`; the pause blur's tick `sub_825D8E68` is called from
  `sub_825D9168` and the menu key repeat from the input channel update, so their 30 Hz stays assumed (not measured).
- **RNG, "shown before", end-of-load black (2026-10-25, USER-ASKS 334; research
  research b588, verified the reset and the hold in the recomp code):**
  - RNG [code]: the tips object builds its own generator once (constructor `sub_82605860`, T+244; `next()` =
    `sub_82970628`, a cascaded-sum generator over six words), seeded from the shared game generator `sub_82A8AF10`
    (fixed start words in the image, about 100 callers). Once per boot, never per load; the first picks differ boot to
    boot because the shared sequence has moved on [trace]. Ours (a clock-seeded per-boot LCG, kept across loads) gives
    the same outcome, so it stays.
  - "Shown before" [code]: `sub_82605AA0` clears T+202..207 on every open of the screen (6 x `stbu` from T+202); the
    counts and the last shown panel stay. Ours never cleared them, so after a few loads every panel sat in the
    "shown before" group and the least-shown rule stopped working. Fixed: `TipsLoader::open` clears them (test
    `each_open_forgets_shown_before_but_keeps_the_counts`). The last Coach Frank / hint line globals (0x830CEBDC /
    0x830CEBE0) are per boot and written only by a pick: `pick_line` now leaves the previous line alone for a list
    of one or none, as retail.
  - End-of-load black [code + trace]: the loader state `sub_82709338` holds the fader black at 0.04 on enter (not the
    tips screen), closes the screen once it is black and the screen is up (no outro wait), and `sub_82708D00` releases
    the fade when another thread's counter (`[[0x83083BCC]+0x50]`) has moved 6 past its value on enter (`addi r6,r10,5`
    / `cmplw`); the recomp shows 2 to 3 FE updates from close to release. Ours held about 5: `BLACK_FRAMES` is now 0,
    so the release comes 2 updates after the close.
  - Still open: which thread drives that counter on the console; `sub_82709068`'s branch on byte 0x830B7C2B that skips
    the release; T+300 / T+304 (every third open sets a flag, likely the online fetch throttle).
- **Startup load: NOT RETAIL (ours, by design):** see "Startup: not shown, by design" below.
- **Stats panel (2026-10-25, USER-ASKS 334):** filled from the player's career stats, see "Career stats" below.
- **Game run (2026-10-24, scripted `SKATE_MAP_SWITCH` BlackBoxPark -> University, the scratch menu set with the movie):**
  request 39.03, black 39.86 (0.83 s, `fe_fade` then at 30 updates), FE 52 open and its intro done at once (clear 40.03),
  commit 43.51, slices done 43.98, black 44.96, close, clear 45.99 (`RETAIL_MENUS fade` / `open loading screen` log
  lines). The screenshot 3.5 s in shows Coach Frank's panel with a disc line over the movie's own dark backdrop, laid
  out like the recomp's screen (a local screenshot). Fixed on the way: the PC
  overlay forced itself open on every transition, so the stage never saw a load without it; it now stays closed when
  the front end has its loading screen (`LoadingCover::available`).
- **Differences to the recomp shot (2026-10-24 evening, USER-ASKS 326):** a full-size recomp shot of the loading
  screen (scripted Challenge Map teleport, `SHOT_FULL=1`, local screenshots) settled them:
  - **Screen edge offset.** "Loading..." sits at x 100 of 1280 in the recomp, 236 in ours: the movie places `mLoading`
    at 98 + `Screen_EdgeOffset`, and our boot sent `SetScreenEdgeOffset "136"`. Retail sends that only when a display
    flag byte is 0 (`boot-enums.md` row 7), and the video calibration movie picks its SD pages when the offset is
    above 0, so 136 is the SD (4:3) safe frame; on the HD screen the recomp shows, the offset stays 0, `main.apt`'s
    initial value [trace + code]. Fixed: the boot no longer sends it. This moves every screen that reads the offset
    to its HD place: the chyron (`_x` = offset), panels (`panel.apt`, Free Play options), tabs, the Trick Guide,
    credits, the action overlay, the autosave wheel and the FMV popup. The Trick Guide panel's right edge was about
    200 px further in than in a retail shot (local screenshots); 136 of that was this.
    It also answers the video calibration question below: retail shows the HD pages.
  - **Colours: no difference.** At full size the text matches (body (111,147,163) recomp vs (112,147,160) ours, title
    (54,78,101) vs (65,87,110), backdrop (20,27,39) vs (20,26,40)); "dimmer" and "bluer" came from the 480 px JPEG.
  - **Spinner.** The blue wheel at the bottom right is the autosave layer's load tag (`controls/autosave`, one of the
    layers `main.apt` loads): `_global.Load_EnableTag(bEnableBackground)` -> `SetLoadingVisible(true)` (corner
    position, "load" frames, wheel visible, animation on), `_global.Load_HideTag()` hides it. In the recomp shots
    it is up exactly while the loading screen is (26.5 to 31.3 s of the run). Ours now calls `Load_EnableTag(true)`
    when FE 52 opens and `Load_HideTag()` when it closes (`retail_menu_stage::load_tag`); a menu set without the
    layer runs without it. Retail's own caller is the main table native EnableLoadingTag `825A9EC0` (not traced).
- **Moddability (2026-10-24, USER-ASKS 326):** `sdk.loading` (capability `loading` 1, `sdk/ENGINE_API.md` "Loading
  screen"): a mod's hint and Coach Frank lines by key (string ids localised, other text as written), joined to the
  retail lines when the screen opens; `retail(false)` leaves only mods' lines on a panel mods fill; `hold` / `release`
  keep a running load's screen up past the engine's own work, at most 30 s (`tips_loader_host::LoadingMods`,
  `MAX_MOD_HOLD_SECS`); all of it goes with the mod (`retail_menus::clear_owner`). The movie is replaceable through
  the menu movie overrides. Panel timings and events (2026-10-25, USER-ASKS 334): `sdk.loading.timing({show, outro})`
  (1 to 120 s / 0 to 10 s, retail 9.0 / 0.5; the newest mod wins, gone with the mod; `LoadingMods::timing`, set on the
  screen when it opens) and `loading_opened` / `loading_closed` on every mod's `on_event` with the map being loaded.
- **Loads picked in the PC overlay (2026-10-25, USER-ASKS 334):** they kept the overlay open with its progress text
  (the known issue "loads started from the PC menu still show the PC menu's progress text"). The overlay now closes
  when a load starts and the loading screen covers it like any other load (`graphics_menu` transition branch,
  `retail_menu_stage` load step); a menu set without the movie still gets the overlay's progress text.
- **Startup: not shown, by design.** Retail's boot load uses the same screen; ours shows its native splash for the
  engine start (before mods, config and the menu set load) and opens the game window only when the first world is
  playable (doc 31 "Startup loading screen"), so there is no load left for the tips loader to cover (b581 3c
  option (i)). NOT RETAIL (ours).

## Career stats (2026-10-25, USER-ASKS 334)

The loading screen's Stats panel shows the player's career stats, as retail does offline. Research
research b589 (verified the table rows, enabled bytes and labels in the image and
`shared.json`, and read the time, distance and ped-hit code below itself).

- **Retail [code + data]:** the Stats object `[0x83067094]` (vtable 0x822FDB30) points at a 65-row table (TU3 0x83043AE8,
  12 bytes: enabled byte, label string id `ID_RECORD_*`, float); rows 0 to 46 are enabled, 47 to 64 not. The setter
  vt+0x18 (0x8250DFF0) adds to a row (career running totals; rows 61 and 64 are set). The panel's offline fetch
  (sub_82607840) keeps enabled rows whose value is not 0, shuffles them and keeps 16; GetStats(5) shuffles again and
  takes 5. The formatter vt+0x14 (0x8250D6D0) by row: time (`hh:mm:ss.ff` from `ID_COMMON_TIME_FORMAT`, 0x8290EB38:
  a field shows when it or an earlier one is not zero, padded to its width, hundredths only without hours; then
  " s"), distance (`{0:distance.1}`: the handler 0x824FE848 registered under "distance" by 0x824FDFC0 shows the number
  with 1 decimal and " m", or times 3.0 and " ft" when Game Settings > Units is imperial), percent (row 17: row 16 over
  row 17), the ped type hit most (row 46), money, grouped integers.
- **Ped hits [code]:** 0x826C0730 adds 1 to a hit row by the hash64 of the ped's entity key: `jock01`,
  `teenage_female01`, `teenage_male01`, `adult_male01`, `adult_female01`, `business_male01`, `business_female01`,
  `skater_male01`, `skater_female01`, `tourist_male01`, `tourist_female01`, `security_street01` / `security_nsz01` /
  `security_nsz02`; every other ped counts as a bum (all 14 hashes matched against the exported entity keys).
- **Ours:** setup exports the table with the menu tables (`menu_tables::career_stats`, located by the pointer to
  `ID_RECORD_DMO_01`; TU3 and disc builds checked; setup now requires it, so the environment group runs again on the
  next Refresh Game Files). `skate_core::career_stats` (rows, kinds, the fetch, the formats) and
  `career_stats_host.rs`: the local player's totals in `settings/career-stats.json` (written every 30 s when changed and
  at exit; the 30 s write runs on a short-lived thread, off the game thread). Optimisation pass: a few adds per fixed
  step and a small JSON (under 2 KB) at most every 30 s, nothing per frame to measure.
- **Rows fed (2026-10-25, USER-ASKS 335; research b590, checked the bail-end
  writer 0x8250F820, the trick-end writer 0x82773D98 and the move-end writer 0x82774230 itself):**
  - Time played (8; retail 0x825D8B50 adds the game tick while the game state is 5; ours the fixed step outside map
    loads), distance on the board (7) and on foot (9), skitching distance and time (11, 12; while the skitch state
    holds a car), ped hits by type (47..60, 0x826C0730).
  - Wipeouts (6): +1 per bail start (0x82774DA0; ours: the scoring runtime's bail count). Bail distance (29): position
    steps over 0.005 m while bailing, added in whole metres (0x82775158 / 0x82775018).
  - Trick ends (0x82773D98): successful (24) / unsuccessful (23) tricks per announced trick leaving its slot, and on a
    success a flip (15) or a late flip (18: the trick's base is the ollie, `catalog::base`); the scoring runtime posts
    `StatEvent::TrickEnd` from `finish`.
  - Move ends (0x82774230, completed moves only): invert time (10, a handplant's held time), manual (21), powerslide
    (22) and grind (13) distance from the runtime's move tracker, which is retail's 82DB0588 (path length, the
    forward projection on the slot that uses it, steps over 0.005 m); a move that carries on as the next trick adds
    its distance when it finally ends.
  - Body flips: an attempt (17) on the first tick of a flip (82DA8EB8 latch), the landed count (16) when a landed air
    ends (0x82773C00, |P+0x2674|); row 17 shows the percent of 16 over 17.
  - Pushes (19) and mongo pushes (20) (2026-10-25, USER-ASKS 340; checked 0x82DB6EC0 at 0x82DB7B58..7C18 in the
    recomp code): while the processed state word +2468 (0x9A4) has the pushing bit 25, the push is a mongo push when
    bit 26 equals (animation output +10371 riding switch == +10374 regular stance), else a regular one; each kind is
    pulsed on its rising edge (Q+0xA3 / Q+0xA4, mongo checked first, last tick's kinds latched in +1296 bits 22 / 23)
    and 0x82775328 adds 1 to the row. Ours: `career_stats::PushLatch` on the published `flags_2468` and the animation
    packet's `regular_stance` / `riding_switch`, in `career_stats_host::track`.
  - Hall of Meat bail record, first part (2026-10-25, USER-ASKS 340; research b592,
    checked 0x82DAF1D8 at 0x82DAF624..F70C and 0x82510990 itself): retail's per-skater tracker updates every tick of
    the bail and 0x8250F820 adds it to the rows at bail end. Time in bail (45): ticks while the body is not at rest
    (wipeout `over_599` clear) and the gate `[[skater+8]+452]` is 0 (meaning open; ours: always 0). Push offs (40):
    +1 every tick with the wipeout response strength (+580) above 0, with no other gate (b592 said gated, the code is
    not). Peds (26) and vehicles (28) hit: distinct entities the body touched during the bail, 8 at most (the
    scorers refuse a 9th); ours from the skeleton contact lists `ped_hits` / `vehicle_hits`. Ours:
    `career_stats::BailRecord` from `board_wiping_out` rising to falling (retail's end is the HOM state 4 event
    0x825694D8, NOT RETAIL YET), dropped on a map load; ticks become game time (dt x 60 for counts).
  - Injuries (2026-10-25, research b592 / b594 / b595; checked 0x82DAE6F0, the level loop in 0x82DAF1D8,
    0x82510990 and the record copy in Fill 0x82BD60C8): each tick of the bail, bone i reads the skeleton contact
    force of part i + 1 (bones 23 / 24: the specific force of parts 1 / 23; Fill copies `force` / `specific_force`
    into the record HOM reads), times 2.0 times `scoring_wipeouts` `Hash_B7514DE7EBDE9D01` (layout +0xBFC, 0.75).
    Over 0.1 it adds to the bone's accumulator and spreads along its BoneSet links (factor x impact over 0.1, never
    back to the bone it came from); the peak is kept, the accumulator decays x0.8 per tick; the level is the highest
    of 6 whose threshold is above 0 and below the peak. At bail end each bone adds one injury at its level (0 scrape,
    1 bruise, 2 sprain, 3 dislocation, 4 / 5 broken: rows 33, 32, 31, 30, 3); a level on bone 0 or 23 adds a head hit
    (4), on bone 24 a groin hit (5). Ours: `skate_core::hall_of_meat` (Tuning from the BoneSet field and the scale,
    loaded with the physics settings, data-gated test), fed in `career_stats_host::track`.
  - Bail ground and air time (36, 37; research b596, checked the contact flags in 0x82DAE9A8): a bone touches when
    its record's world flag (+0x73 = our `groups[3]`) is set and its channel normal (Fill copies the normals raw) has
    y over 0.33; the skater is airborne when no bone touches. Each finished air adds its time (an air still open at
    bail end is not added, 0x82DAEBC0); ground time is the time in bail less the air time. Ours:
    `hall_of_meat::airborne`, `BailRecord::air_seconds`.
  - Bail ground and air distance (38, 39; research b597, checked 0x82DAECB8 and 0x82DAEBC0): the body is the
    skeleton centre of mass (retail `[skater+0x24]`, written by 0x82BE1AE8 from Skeleton +16144 / +16176; ours
    `board_frames.centre_of_mass` / `com_velocity`, the same values our port publishes as `reckoning.vector_64 /
    _16`). In an air the apex keeps the highest height and the air's distance is the apex less this tick's height;
    each finished air adds it (a fall height, not a horizontal length). On ground ticks the speed times the step adds
    to the ground distance.
  - Body tweaks, rolls, judo kicks, spread eagle (34, 41..44; research b594 / b596 / b597 / b598, checked 0x82DAC930
    and the tracker gate in 0x82DAF1D8): three trackers on the hips (physical part 23) measure the turn about each
    of its axes per tick (a turn over 90 degrees in a tick counts 0); a tracker's sum is banked as degrees when it
    starts shrinking after more than 30 degrees; the total body rotation is the banked degrees plus the open sums.
    While the body is at rest or retail's cVar2 (PhysOut_Air +452, our `use_air_reckoning_452`) is set the trackers
    re-seed without counting. In the air the wipeout gesture stick (not WipeoutControl) outside the dead zone
    (`Hash_9C494E9614FA5ED6` 0.6) picks the first of the 4 BodyTweak poses within 180 / 4 degrees; holding it adds
    the rotation, time and longest hold to that pose's slot, and a hold reaching the element's frames (15) counts a
    tweak (500 points). Rows: tweaks (34), barrel rolls (41) = floor((pose 3 degrees + 90) / 360), cannonball rolls
    (42) = the same for pose 0, judo kicks (43) = new peds and vehicles hit while the pose (this air's or the last
    one's) is 2, spread eagle time (44) = pose 1's time. cVar2 also holds the time in bail, air frames and ground
    distance (not push offs). Ours: `hall_of_meat::{Rotation, Poses, PoseTable}`, `career_stats::BailTick`.
- **Rows not fed yet:**
  - Objects hit (35): retail counts the distinct placed dynamic props (the DMO census) near the skater whose sticky
    "touched by actor" mask has this skater's bit (filler 0x82761100, scorer 0x82DAFFA0, research b598). Our skeleton
    contacts with props carry no prop identity (`physics/skeleton_feedback.rs`: only network solids, cars and peds,
    become hits), so the row waits for that (todo `hom-objects-hit.md`); judo kicks also leave out the props retail
    counts when they are worth points.
  - Air grab time (14, P+0x266C) and peds / vehicles jumped (25, 27; a merged jumped-over record): no engine source
    yet. Money (62, 63) and the Create A Spot rows are disabled in retail's table and have no store here.
- **NOT RETAIL YET:** retail's shuffle order and money format are not decoded; whether 0x826C0730 and the bail-end
  writer double count a ped hit is open (ours has only the first). Moves over 5 m in one fixed step (teleports) are
  not counted as travel.
- **Mods:** `sdk.loading.stat(key, {label, value})` adds rows to the Stats panel (capability `loading` 2).
- **Tests:** `skate_core::career_stats::tests` (records, clock, formats, fetch), `career_stats_host::tests` (hit rows,
  save and load, panel rows), `menu_tables` data-gated (65 rows at 0x83043AE8, 0..46 enabled), the setup check.

## Startup loading screen (2026-10-15)

**Problem.** Starting the game showed a window that sat frozen for about a minute (the user found the start after a
fresh build very slow and asked for a loading screen or some sign the game is not frozen). In the user's
session log of 2026-10-15 (`game-20261015-215442`) the window opened 7 s after launch and the first frame took 43 s.

**Root cause.** [log] Of those 43 s, about 39 s (03:55:00.6 to 03:55:39.5 UTC) were `game_audio::content::frame` ->
`native::launch` reading and decoding the AEMS projects and the player's banks inside the first frame, so nothing
could draw. The launch-time asset check ran in parallel on its own thread for 31 s; the audio start's player banks
finished 0.5 s after it, so on an HDD the two competed for the disk.

**Change.**
- `game_audio::native::begin_launch` / `finish_launch`: the first start without audio overlays builds the runtime on
  an `audio-start` thread from its own `Library::load` of the install (the startup `Library` is the same call on the
  same root), then installs it on the main thread exactly as `launch` does. `AudioContent` gets a `Starting` state;
  restarts and the first start with overlays stay synchronous.
- `splash.rs` (new): a small native splash window (its own thread and message loop) shown from the first moment
  of the process, before config, skater, physics and world loading. A skateboard with no skater rolls across it,
  the current step and item are written under it ("Loading sound: sound bank grind_concrete (12 s)"), and the last
  finished steps are listed dimmed below, like RPCS3's progress lines. `main.rs` names each startup stage on it;
  any thread reports the item being read (`startup_loading::detail`). `SKATE_NO_SPLASH=1` turns it off.
  The board (2026-10-16) is drawn as a side view: kicked nose and tail, griptape over a maple edge, two trucks,
  urethane wheels and a ground shadow, drawn at 4x and scaled down so the edges are smooth;
  `SKATE_SPLASH_BOARD_BMP=<path>` writes it to a BMP to look at without starting the game.
- `startup_loading.rs` (new): the game window is created hidden (`app.rs`, `visible: false`) and shown and brought
  forward when every hold is released; the splash closes then. Gameplay waits while a hold is up (virtual time
  paused, after the menu's own pause rule), so the start runs in the same order as before. The audio hold is
  released when the first start is done or failed, at once without an audio install; a hold that hangs gives way
  after 180 s so the window never stays hidden. Log: `STARTUP_LOADING done in <ms>`.
- `asset_check.rs`: the check starts once the loading phase is over instead of at startup.

**Not retail.** Retail boots into its own movies and shows its tips loader (FE screen 52, "Loading screen" above)
for in-game loads; the splash only covers our engine's start. Engine systems or mods hold the loading phase with
`StartupLoading::hold` / `release` (the label is the splash's step). The splash itself runs before mods load, so
its look is not moddable. Open: a Lua SDK entry for the hold.

**Verification.** `startup_loading::tests::holds_show_the_first_label_and_end_when_all_are_released`; the data-gated
`game_audio::content` tests (all 4 pass; `game_world` now starts through the thread and waits for it);
`splash::tests::steps_and_items_become_finished_lines`. Live run 2026-10-15 (warm file cache): splash up at 1 s
("Reading the settings and the map"), "Building the world, menus and HUD" with the finished steps at 6 s, game
window shown at 10 s with the splash gone; `STARTUP_LOADING done in 5170 ms`, no errors.

## Files
- Trick Guide camera from the recomp trace: `crates/skate-data/src/ice_camera.rs`, `tests/ice_camera.rs`,
  `crates/skate-core/src/camera_track.rs`, `crates/skate-game/src/trick_guide_scene.rs`, `retail_menu_stage.rs`.
- Trick Guide camera, fog and sun: `crates/skate-core/src/camera_track.rs` (new), `crates/skate-data/src/ice_camera.rs`
  (new), `crates/skate-data/tests/ice_camera.rs` (new), `crates/skate-data/src/trick_guide.rs`,
  `crates/skate-core/src/trick_guide.rs`, `crates/skate-mods/src/menus.rs`, `crates/skate-game/src/trick_guide_host.rs`,
  `crates/skate-game/src/trick_guide_scene.rs`, `crates/skate-game/src/retail_menu_stage.rs`,
  `crates/skate-game/src/retail_sky.rs`, `tools/asset_pipeline/scene_clips.py`.
- Trick Guide demo: `crates/skate-core/src/animation/raw.rs` (new), `crates/skate-data/src/scene_clip.rs` (new),
  `tests/scene_clip.rs`, `tests/trick_guide_checklist.rs` (new); `crates/skate-game/src/trick_guide_scene.rs` (new),
  `retail_menu_stage.rs`, `map_render.rs`, `retail_sky.rs`, `app.rs`, `main.rs`; `tools/asset_pipeline/scene_clips.py`,
  `asset_exports.py`, `versions.py`, `tools/prepare_trick_guide_scene.py` (new).
- Trick Guide: `crates/skate-core/src/trick_guide.rs` (new), `crates/skate-data/src/trick_guide.rs` (new),
  `menu_tables.rs`, `collections.rs`, `tests/trick_guide.rs` (new); `crates/skate-game/src/trick_guide_host.rs` (new),
  `retail_menu_stage.rs`, `crossbar_host.rs`, `apt_movie.rs`, `apt_stage.rs`, `apt_text.rs`, `apt_scene.rs`,
  `fe_backdrop.rs`, `skater_animation.rs`, `retail_menus.rs`, `modding/mod.rs`, `main.rs`; `crates/skate-mods/src/menus.rs`,
  `vm.rs`, `api.lua`; `sdk/skate.lua`; `tools/prepare_menu_movies.py`, `prepare_hud.py`,
  `asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Out-of-date prompt: `crates/skate-game/src/asset_check.rs` (new), `setup.rs`, `graphics_menu.rs`, `graphics_menu/view.rs`
- Stored settings: `crates/skate-game/src/retail_menus.rs` (`StoredSettings`, `setting`), `scoring_hud.rs`
- Swap-in: `crates/skate-game/src/retail_menu_stage.rs` (`enabled`, `RetailPause`), `graphics_menu.rs` (`overlay_key`)
- PC overlay: `crates/skate-game/src/input/platform.rs`, `graphics_menu.rs`, `graphics_menu/view.rs`, `fonts/`
  (Inter + OFL), `fe_backdrop.rs` (overlay blur), `scoring_hud.rs` (HUD hidden), `customiser.rs`
  (`Navigation::held`), `game_audio/mod.rs` (volume row accessors), `custom_difficulty.rs`, `retail_menu_stage.rs`
- Sub-options: `crates/skate-data/src/menu_tables.rs` (`SubOptionRow`), `tests/menu_tables.rs`;
  `crates/skate-game/src/crossbar_host.rs`, `retail_menu_stage.rs`; `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Button prompts / panel: `crates/skate-game/src/prompt_glyphs.rs` (new), `input/controller_kind.rs`,
  `input/platform.rs`, `input/tests/controller_kind.rs`, `apt_scene.rs`, `apt_movie.rs` (`clip_matrix`,
  `transform_changed`), `apt_stage.rs`, `hud_render.wgsl`, `scoring_hud.rs` (`render_draws` gamma flag),
  `retail_menu_stage.rs`
- Game Settings screen: `crates/skate-game/src/settings_host.rs` (new), `crossbar_host.rs`, `retail_menu_stage.rs`,
  `retail_menus.rs`, `fe_backdrop.rs`, `apt_vm.rs`, `apt_movie.rs`, `apt_stage.rs` (test), `main.rs`;
  `crates/skate-core/src/fe_input.rs` (retail repeat ticks); `crates/skate-data/src/menu_tables.rs`
  (`settings_titles`), `tests/menu_tables.rs`; `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Pad input: `crates/skate-core/src/fe_input.rs` (new), `lib.rs`; `crates/skate-game/src/retail_menu_stage.rs`,
  `graphics_menu.rs` (Start / Escape gating)
- Screen stack: `crates/skate-core/src/fe_stack.rs` (new), `lib.rs`; `crates/skate-game/src/retail_menu_stage.rs`,
  `fe_backdrop.rs` (index, info), `graphics_menu.rs` (`fe_suspended`), `game_audio/mod.rs`;
  `crates/skate-data/src/menu_tables.rs` (record fields), `tests/menu_tables.rs`
- Row handlers: `crates/skate-game/src/retail_menus.rs` (`OVERLAY_ITEMS`), `retail_menu_stage.rs`,
  `graphics_menu.rs` (`open_section`)
- Pause blur: `crates/skate-core/src/fe_blur.rs` (new), `lib.rs`; `crates/skate-game/src/fe_backdrop.rs`,
  `fe_backdrop.wgsl` (new), `retail_menu_stage.rs`, `retail_exposure.rs` (label), `retail_menus.rs` (cleanup,
  snapshot), `modding/mod.rs`, `app.rs`, `main.rs`; `crates/skate-mods/src/menus.rs`, `vm.rs`, `api.lua`;
  `sdk/skate.lua`;
  `crates/skate-data/src/menu_tables.rs` (`FeScreen`, `screens`), `tests/menu_tables.rs`;
  `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Milestone 6: `crates/skate-core/src/crossbar.rs` (new), `lib.rs`; `crates/skate-game/src/crossbar_host.rs`
  (new), `fe_language.rs` (new), `retail_menu_stage.rs` (new), `apt_stage.rs`, `apt_scene.rs`, `apt_movie.rs`,
  `apt_vm.rs`, `retail_menu_movies.rs`, `scoring_hud.rs` (`render_draws`), `app.rs`, `main.rs`;
  `tools/prepare_menu_movies.py` (set v4), `tools/vendor/skate3_ui/actions.py` (getURL);
  `crates/skate-data/src/menu_tables.rs` (`ScreenTemplate`, `templates`), `crates/skate-data/tests/menu_tables.rs`,
  `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Milestone 5a: `crates/skate-game/src/apt_stage.rs` (new), `apt_movie.rs`, `apt_vm.rs`, `main.rs` (module)
- `crates/skate-data/src/menu_tables.rs` (extractor, JSON, conversion), `crates/skate-data/tests/menu_tables.rs`
- `crates/skate-core/src/menus.rs`, `menus_tests.rs` (registry), `menu_values.rs` (value rules, milestone 2)
- `crates/skate-game/src/apt_vm.rs` (milestone 3a opcodes, tests)
- `crates/skate-game/src/apt_imports.rs` (milestone 3b import resolver, mod overrides, tests), `apt_movie.rs`
  (`link_imports`), `main.rs` (module)
- `crates/skate-game/src/apt_imports.rs` (milestone 3b import resolver, mod overrides, tests), `apt_movie.rs`
  (`link_imports`), `main.rs` (module)
- `crates/skate-game/src/retail_menus.rs` (resource, loader, mod commands, events, snapshot, engine value bindings),
  `game_audio/mod.rs` (`RetailVolumes`, saved), `game_audio/native.rs` (Master.in1..3), `main.rs`
  (`--extract-menu-tables`), `app.rs`, `modding/mod.rs` (commands, cleanup, events, snapshot)
- `crates/skate-mods/src/menus.rs`, `vm.rs` (commands, capability), `api.lua`; `sdk/skate.lua`
- `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`, `asset_exports.py`, `versions.py`
- Milestone 3c: `tools/prepare_menu_movies.py`, `tools/test_prepare_menu_movies.py`, `tools/prepare_runtime_huds.py`;
  `crates/skate-game/src/retail_menu_movies.rs`, `retail_menus.rs` (startup, cleanup, snapshot), `modding/mod.rs`
  (`menu_movie`), `apt_imports.rs` (`main_checkout`, `OverrideTable::position`), `apt_vm.rs` (test path)
- Milestone 3d: `crates/skate-game/src/apt_display.rs` (`ClipAction`, `clip_event`), `apt_movie.rs` (`Pending`,
  dispatch, `next_action`, link), `hud_runtime.rs` (drain), `apt_vm.rs` (0xA6), `retail_menu_movies.rs` (set
  version 2, test); `tools/prepare_menu_movies.py` (`clip_actions`), `tools/test_prepare_menu_movies.py`
- Milestone 3f: `crates/skate-game/src/apt_display.rs` (clip-depth storage rule), `apt_movie.rs` (test),
  `retail_menu_movies.rs` (clip-depth count in the set test)
- Milestone 4: `crates/skate-game/src/apt_scene.rs` (`Fill`, `ShapeSource`), `apt_movie.rs` (`shape_origins`),
  `retail_menu_movies.rs` (shapes, `MenuScene`, `image`, mod bitmaps, tests), `scoring_hud.rs` (optional texture),
  `modding/mod.rs` (`menu_bitmap`); `crates/skate-mods/src/vm.rs`, `api.lua`; `sdk/skate.lua`;
  `tools/prepare_menu_movies.py` (set version 3), `tools/test_prepare_menu_movies.py`

## Verification
- `cargo test -p skate-data --test menu_tables -- --ignored` (local files, skipped when absent): the TU3 image gives
  exactly the documented tables and addresses (36 rows, 13 screens, 5 categories, 49 items, 56 keys, 9 modes,
  Career Main, park rows 9 slots, Free Play 4 tabs); the user's disc `default.xex` (a different build) gives the same
  content at its own addresses, minus TU3's additions (above), compared entry by entry against TU3.
- `cargo test -p skate-core menus`: greyed by default, a handler enables, dirty-park confirmation intercepts, hidden
  rules (Project10, SFX pack), links and the Video choice, mod add / hide / reorder / relabel and exact revert,
  owner stacking.
- `cargo test -p skate-mods menus`, `cargo test -p skate-game retail_menus`: options and rule validation, mod
  commands end to end, events JSON, revert.
- `python -m unittest tools.asset_pipeline.test_menu_tables`: the setup step through the real spawn path.

## Open questions
- Decoded 2026-10-29 (research b673): Career IsItemEnabled `8261F850` greys SkateReel / SkateWith when `82511168`
  says the current world row has `IsSkatePark`; EnterSkatePark while ProgressionManager +0x3A is set and neither DLC
  CCF0003 nor the `film_skpk_univ_01` challenge is known; PartyPlay without the San Van pack (CCF000C, `82518AD0`).
  The Free Play twin is `82620320` (only SkateReel / SkateWith, same world check); `826205C8` is the Free Play PARK
  twin of `8261FB18` (SavePark = flag `[0x83084690]+0x23` only; RatePark / FlagSkatePark = world state 11, flag set and
  `82516C18` returning 0). Ported as retail rules on new facts (`world_is_skate_park`, `skate_park_locked`,
  `party_pack_owned`; skate-core menus 11 pass). Open: the game publishes no menu facts yet (context always
  "FreePlay"), so the world fact needs the current map's world row; PartyPlay stays greyed until the pack fact is
  published from the DLC unlocks.
- Mode keys are ours (retail has no names); the online / lobby menus are extracted but not decoded further (online
  is a custom implementation later).
- Retail stops the settings row count at a hidden row; the registry skips hidden rows instead (identical for the only
  retail case, the SFX pack row, which is last on its screen).
- Next milestones (settings model done, above): APT player gaps, host side (Game / CoreMenu objects, fe
  sounds, input repeat), then swapping in the retail menus with the current features as hooks.

## Challenge Map: movie export and hooks for every map node (2026-10-18, in progress)

Todo (user): implement the retail challenge map teleports (DownTown, University and so on); like the rest of
the retail menus, unusable menu items are greyed out for now, and every menu item gets its hooks even if nothing
uses them yet.

Retail (research b244 to b244d, b255; Ghidra + the exported movie): one movie `source/screens/map/challengemap`
(template 68, FE screen 17) over the host object FEChallengeMap, whose 24 natives (`sub_8264A8A8`) are all getters
(GetStuffMenuItems returns [count, selected, labels, enabled, icon 1, icon 2]; several are retail stubs). The tree is
vault data: `map_listing` (tabs and folders by parent), `map_filter`, `map_category`, and leaves of the classes
`fe_locations` (teleports), `world`, `challenges`, `map_commands`, `code_unlocks`. Offline modes (career and free
skate) open the root `progression`, online `online` (`sub_8261BC30`). An fe_locations leaf teleports to its locator
matrix unchanged, off the board; it loads only when the target is outside the streamed area (30 / 50 m).

Done so far:
- Export: `tools/prepare_menu_movies.py` adds the movie (format version 6, so installs refresh); the data-gated
  export test exports 35 movies with every import and no unresolved shapes.
- Hooks: `skate_core::menus::EntryKind` has one kind per vault class (`map_listing` ... `code_unlocks`), and
  `MenuData::map_nodes` holds the tree (entry, parent, label, sort order). Every node is an entry from the start, so
  each has its hooks (handlers, visible / enabled rules, interceptors) with the hook id `<class>:<key>`
  (`fe_locations:dist_downtown_aletown`). Greying is the existing rule: no handler, greyed; a tab or folder is
  enabled when something inside it is. `Hide` edits drop a node with everything under it. Test
  `challenge_map_nodes_have_hooks_and_stay_greyed_until_handled`.

Next: the tree loader from the collections + teleports.json, the FEChallengeMap host object, the off-board teleport
handler (`fe_locations`), the pause menu row switching to the screen when the movie is installed.

Progress (2026-10-18, evening): the vault tree loads into the menus (`skate_data::challenge_map`, Downtown's 14
locations; retail's location filter Lua as data), every location with a teleport matrix gets a `teleport:<key>`
engine handler, the navigation core (`skate_core::challenge_map`) and the FEChallengeMap host (24 getters) are in,
teleports land off the board (`travel_ex`, `challenge_map::plan`: same map instant, another map through the map
transition, refused in multiplayer). The pause menu's Challenge Map opens FE 17 with the retail movie whenever the
installed set has it (the PC overlay's MAPS stays the fallback for older sets). The movie loads since the instance
guard went to 16384 (it pre-allocates 11 picker rows, ~2838 clips; research b260). GetStuffMenuItems list 3 is the
header flag (`!(child vt+0x28)`), not the greying: the movie adds a `true` row as a picker header (bytecode of
UpdateStuffMenuItems: `Not; If` jumps to `AddItem` when false, else `AddSpecificItem(text, mHeaderItem)`), and the
picker skips headers when it places its highlight; sending the greying there made Downtown a header and A landed on
the greyed Industrial row. Greyed rows come from `Game.OnMenuGetItemEnabled` only. Test
`retail_challenge_map_screen_opens_and_picks_a_location` (data-gated) opens Downtown and picks its first location
through the movie's picker. B returns to the folder's row and each tab keeps its row (research b261: pop
`sub_82D0D378` selects the folder it left; the per-tab slot `mgr+0x118 + 4*tab` saved by `D100` at the root and
restored by the load step `82D0D728`; both checked in the recomp).

Map pictures (2026-10-18, research b257 / b258 / b259 / b262): menu set version 7 exports the 18 map pictures
(`fe/source/images/map/*`, fedynamic.big, a second narrow extraction) plus `cursorchallengemap` as named runtime
images (manifest `images`: request key -> RGBA payload, size; `retail_menu_movies::image_key`). The movie loads them
itself: `image_renderer`'s CONSTRUCT clip action sets `_type = "ImageRenderComponent"`, `LoadRX2Texture` reserves a
slot through `Game.ImageRenderer_ReserveNextFreeIndex` and renames the clip to the slot index (b262). The 40 global
slots and the natives are `image_renderer_host::ImageSlots` (Reserve / Free / Load / LoadAssetByName / Unload /
LockAspectRatio as b258 section 1); the scene draws an `ImageRenderComponent` as one quad `slot/<n>` over its
placeholder and the stage resolves it to the slot's image; an empty slot or an unknown image draws nothing (retail),
never the white fallback. Which picture: the highlighted location's `world` record tMapInfo name. The menu
export resolves each world record's own name (word 0 of `Sk8::FE::tMapInfo` = the name's offset in
skatercollections.bin, read through `vlt.vault`) into the manifest's `world_maps`, and
`skate_data::challenge_map::location_maps` walks location -> district world -> parents. (A first version put this in
the vault converter `vlt.py`; that file is in the core, living world and customiser fingerprints, so every install
would have redone those stages: moved to the menu stage, which re-runs for version 7 anyway.) The icon pack
`fe/source/images/icons/minimap` is exported per texture (`texture/<name>`, 43) for the markers. Folders, the root and
`map_portcarverton` use the default `map1024`; a change posts SetState 34. Mods: `RetailMenuMovies::set_named_image`
replaces a retail picture or adds a name, cleared on mod disable; `MenuData::map_pictures` is data. Markers (research b263 / b264 /
b265, 2026-10-18): in a list of locations each location gets a marker, the highlighted one drawn last. Data per
location (`skate_data::challenge_map::location_markers`, `MenuData::map_markers`): the icon = the location's `Icon`
RefSpec (inherited; every career location: `teleport` -> fe_map_icons name `mini_teleport`), the point = the
locator's x / z (the teleport matrix row 3, the same locator lookup as the teleport) plus the location's `World`
record's Vector2 offset (Downtown (225, -175), Industrial (1000, 1300), University (1100, -400), Skate School (60, 5),
others (0, 0)), projected with that world's tMapInfo bounds (`MapMarker::projected`, `sub_8268DB68`: size b5 x b6
over b2..b1 / b4..b3, 99% / 95%). The movie's `MapRenderComponent` (a CONSTRUCT clip action sets its `_type`, b262)
draws as a placeholder carrying its transform; the stage expands it into 32 x 32 quads (`8268D960`) textured from the
icon pack `texture/<icon>`; an icon the pack lacks draws nothing (retail draws `missingtexture` from
globaltextures.rx2, not exported). The cursor (`IsCursorVisible` / `GetCursorCoordinates`) sits on the highlighted
marker; the movie places its own cursor clip. Not yet: the park screenshot (`LoadAssetByName`), challenge markers
(their locator field 0xF1C6D5FA4A23951E; challenges are greyed anyway). Research files: b257 / b258 / b259
(fedynamic.big map textures, the image renderer natives, the export plan).

Out-of-date menus force the refresh (user, 2026-10-18): the asset check's menu movie / table findings restart the game
into Refresh Game Files once (`asset_check::force_menu_refresh`); a run the refresh started never forces again.
The engine still reads a version 5 movie set (`OLDEST_VERSION`), so a refresh that could not run leaves the menus
working.

## The pause menu no longer dies after Challenge Map teleports; a stopped stage restarts (2026-10-18)
- **Problem** (laptop session 2026-10-18 22:01): after four Challenge Map teleports the next Start logged "APT
  execution/object budget exceeded", the stage stopped, and every later Start opened the PC overlay.
- **Root cause** (research b281, confirmed by a stage test): the teleport closes
  `[map, pause]`; retail's stack op 5 closing a screen below the top unwinds to it and reopens it, so one stack batch
  opened the pause menu and closed it again. The open only queues the movie load; the script's `CloseScreen` ignores a
  screen that is still loading (no unload call reaches the stage), so the crossbar loaded into its holder on the next
  update and stayed rooted: about 1545 objects and one level per teleport, until the VM's 16384-slot guard tripped.
- **Change:**
  - `retail_menu_stage.rs` `drop_reopen_and_close`: an Open and a later Close of the same screen in one batch are
    both dropped (the screen ends closed either way; other effects keep their order). NOT RETAIL (mechanism), same
    outcome.
  - `apt_stage.rs` `unload_into`: a load still queued for a clip being unloaded is cancelled (any movie, mods'
    included).
  - Collector: triggers on slots in use (`live_objects`), not the slot vector's high-water mark (which made it
    collect every update once passed), and `collect_at` is capped at 3/4 of the guard.
  - `apt_vm.rs`: the two limits have their own messages ("APT execution budget exceeded (100000 instructions in one
    update)", "APT object budget exceeded (slots S, in use L)").
  - `restart_failed_stage`: a stopped stage is rebuilt with the startup boot once gameplay is back (up to 3 times
    per session, "RETAIL_MENUS stage restarted (n/3)"); dropping the old stage frees every object its VM held.
  - Log: "RETAIL_MENUS APT objects in use L slots S collect_at C levels N [key:instances, ...] before K screen
    change(s)" at every stack batch.
- **Tests:** `a_screen_reopened_and_closed_in_one_batch_is_neither`; the existing `pause_menu_reopens_without_growing`
  passes (912 live after each of 8 opens). Before the fix, a same-frame open and close grew by about 1545 objects and
  one level each time (2485 to 13300 over 8 cycles). skate-game 807 pass.
- **Next session:** the APT objects line should show the same "in use" after each pause close and no level growth
  across Challenge Map uses.

## Challenge Map markers and cursor at the map clip's origin (2026-10-18)
- **Problem** (laptop session 2026-10-18): "map picture in wrong place, markers in wrong spots, generally the menu did
  not match retail."
- **Retail** (research b282, [code]): MapRenderComponent's drawer
  `825E2618` places markers at the origin `0x830B8600` plus each projected point, 32 x 32 screen pixels. That origin
  is row 3 of the matrix at `0x830B85D0`, which the render callback `825D4390` writes from the clip's 2D matrix (the
  four vector stores, row 3 at +48, checked in the recomp): the clip's translation only, not its scale. Earlier
  research (b263 / b265) read the block as a constant zero. GetCursorCoordinates is the projected point plus the same
  origin; the movie sets `mCursor._x / _y` on the root with no offset. The projection itself (`8268DB68`) already
  matched.
- **Change:** markers are drawn at origin + point, 32 x 32 (before: through the full clip matrix, scale about 2.04 x
  1.51, so spread out and drawn 65 x 48); `ChallengeMapHost::origin` is set while drawing and GetCursorCoordinates adds
  it (before: the cursor sat near the top left). Log "CHALLENGE_MAP markers at origin [x, y]: n markers, first (icon,
  screen point), picture (name or none = the default map)" when the origin changes.
- **Open:** the picture. Its placement rule matches the movie (`mMapImage` 512 x 512 at (572, 120.5),
  `mDefaultMapImage` 1024 x 1024 at (291, -41), stretched, no aspect lock); b282's likeliest cause is the default map
  showing whenever the highlighted row has no picture (`IsUsingDefaultMap`). Decided 2026-10-29 from that line over
  the sessions since: rows without a picture show the default map (picture None, ~560 lines) and the Danny Way row
  shows `map_danny`, as the movie's rule says; nothing to change.
- **Tests:** skate-game 807 pass. Not play-tested.

## Standalone skate-data examples build again (2026-10-19)

Problem (PR #52 next item 7): `cargo check -p skate-data --examples` failed in two examples that include skate-game
sources by `#[path]`. `apt_data` called `apt_vm::Vm::construct`, folded into the VM's opcode handling upstream
(`ddc30284`, 2026-09-14). `scoring_flow_data` set `Frame::nollie` (the field is gone; `regular`, `player_basis`,
`board_basis`, `reckoning_up` and `front_flip` were added) and its included `scoring_runtime.rs` / `apt_scene.rs` now
use `bevy::log::info!` and `crate::image_renderer_host`.
Change (examples only, no engine code): `apt_data` builds the instance itself (`construct`: plain object with the
class prototype, then `Vm::call_function`); `scoring_flow_data` fills the new `Frame` fields with neutral values,
copies `hud_data`'s `image_renderer_host` stand-in and drops the log lines through `extern crate self as bevy` plus a
local `log::info!` that expands to nothing. Verification: `cargo check -p skate-data --examples` clean (dead-code
warnings only), `cargo test -p skate-data` all pass. Running the examples needs the user's exported data (not run).

## Skate Profile (FE 61) (2026-10-19)

Problem (PR #52 next item 8, user 2026-10-19: port FE 61 now): the Free Play pause row Skate Profile opened our PC
overlay's SKATER section; retail opens FE screen 61, the career view.

Root cause / evidence (research b308; re-read the input handler):
- FE 61 (vtable 0x82305448) opens template 106 OfflineProfile, `profile/offline_skateProfile` (info record
  0x830265E0, key `f17ccf55ee91b4f7`). The movie has three tabs (progression, stats, personal bests) and reads its
  pages from two host objects: Progression (registrar 0x825AFBA8: GetOverallCareerData, GetOverallChallengeData,
  GetMilestoneDotData, GetSummaryGroupData) and Stats (registrar 0x825AB630: GetStatsChunk, GetPersonalBestChunk,
  GetCareerProfileSelectedTab = the screen's tab index +0x3C).
- Init 0x8264C530: tab 0, then the refresh 0x8264C830 (tab 0 / 1 / 2 -> SetState 5 / 10 / 20).
- Input 0x8264C608 (skate3_recomp.24.cpp:10977): Back plays `core_b_button` and pops the screen (consumed); the tab
  keys 7 / 10 play `core_tab`, step the tab clamped to 0..2 (at either end the index stays, :11137-11150), then
  SetState(30) and the refresh, also at either end (:11153-11173); Up / Down / LStick up / down play
  `profile_career_scroll` and return 0, so the movie scrolls its list. The three sound keys resolve by the install's
  `fe` records: 0x4D927CD64CC8B818 core_b_button, 0xFD87566338C118E1 core_tab, 0x16469A2827B77C98
  profile_career_scroll. Y (gamercard) and X (achievements) are console platform UI.

Change:
- `skate_core::offline_profile`: the tab index, keys and states as data (`ProfileRules`: tab labels and states, the
  sync state 30, the three sounds; retail's the defaults) and the page data (`ProfileData`: two overall bars, the
  stats and personal-best rows by stable id).
- `offline_profile_host.rs`: answers Progression and Stats from `ProfileData`; `ProfileMods` layers mods' changes.
- `retail_menu_stage.rs`: FE 61 open / close, key routing, the open state posted once the movie loaded; the pause row
  pushes the screen when the movie is in the set (stacked over the SKATER overlay handler, which stays the fallback
  for older sets). `tools/prepare_menu_movies.py` exports the movie (its `table` control comes with it).
- Mods: `sdk.menus.profile(page, key, options)` (`career` / `challenges` bars: title, icon, current, total;
  `stats` / `personal_bests` rows by key: label, value, order, hidden); removed when the mod stops.
- NOT RETAIL YET: the engine has no career, challenge or stats records, so the pages open empty (bars 0 / 0, no rows,
  no milestones or summary groups) until a career system or a mod fills them. The Skater overlay section
  (difficulty, tuning, camera, look) loses this row as its way in while the retail screen is available.

Verification: `cargo test -p skate-core offline_profile` (4: open page, tab steps and both ends, Back / scroll,
a mod tab list); `offline_profile_host` mod layers test; skate-mods command validation;
`retail_skate_profile_screen_opens_and_shows_each_tab` (data-gated: loads the movie, posts 5, 30, 10, 30, 20, 30, 5,
asserts the movie called GetOverallCareerData, GetStatsChunk, GetPersonalBestChunk and GetCareerProfileSelectedTab
and that no Progression / Stats call went unanswered). Not yet looked at in the running game.

Open: GetSummaryGroupData / GetMilestoneDotData entry layouts (the movie's SUMMARY_GROUP_* / DOT_DATA_* constants);
which stats retail lists: answered (doc 31 "Career stats"; 0x8306FD58 is the formatter's 64-byte scratch string, not a
table; the personal bests are the Stats object's +0x20 table 0x830438B8, 35 rows); the career and challenge record keys.

Follow-up (laptop 2026-10-19 12:32): the start-up check said "game files current" on a set without the movie, so no refresh was offered. The menu movie set is now version 8 (`retail_menu_movies::VERSION`, `prepare_menu_movies.py` `VERSION`): an older set forces Refresh Game Files like the Challenge Map's version 6 did; version 7 sets still load (`OLDEST_VERSION` 5) and keep the SKATER overlay until refreshed.

## Extras and cheat codes (FE 18, FE 46) (2026-10-19)

Problem (PR #52 next item 8): the Free Play pause row Extras opened our PC overlay's EXTRAS page; retail opens FE 18,
whose first row takes a cheat code (the zombie cheat). User choices (2026-10-19): cheat entry = "On-screen keyboard";
rows 1-3 (console shoe / pre-sell codes) = "Unlock directly"; Licenses / Credits = "Our licenses + greyed credits".

Root cause / evidence (research b320, `b304-retail-cheat-screen.md`,
`b305-retail-text-entry.md`, `b321-extras-code-unlocks.md`; verified the template and screen records):
- FE 18 (vtable 0x82305390) opens template 15 Extras `main/extras.swf` (info key `62b6afde0ba6567b`). Seven rows
  (table 0x83025EE0): cheat code, Emerica / Etnies shoe code, pre-sell code, credits (FE 25), licenses (viewer FE 3),
  Skate Reel (sub-list, video player FE 24). Natives: Game.Extras_GetNumOptions / _GetOptionTitle / _MenuTitle and the
  picker's OnMenuGetItemEnabled / OnMenuSelect. Input 0x82655658: Back only (core_b_button; Skate Reel -> main rows
  with SetState(10), else pop), never consumed.
- Row 0 opens FE 46 (key `47ddadb752e051b6`): no movie, retail calls XamShowKeyboardUI (title = the row label,
  empty default, max 512, empty allowed, mode 0 = characters from `ALL_VALID_ALPHANUMERIC_CHARACTERS`, warning popup
  0x107 on others and the keyboard again). The callback keeps 32 characters; the matcher compares case-sensitively
  against the cheat table (0x83025F00); `zombie` (id 5) TOGGLES zombie mode in Free Skate class modes, with the
  dialogs ID_VALID_CODE_TITLE + ID_VALID_CODE_CHEAT_ZOMBIEMODE(_DISABLED), refused ID_INVALID_CODE_CHEAT_ZOMBIEMODE,
  wrong ID_VALIDATE_CODE_TITLE + ID_UNLOCK_CODE_FAILED + ID_COMMON_CONTINUE.
- Rows 1-3 (b321): redeeming sets the row's entitlement flag (unlock kind 8) and grants: Emerica = kind 6
  `cac_shoe_emerica_promo` (shoe emerica_reynolds_cruisers), Etnies = kind 6 `cac_shoe_etnies_promo` (shoe
  etnies_skate_3_custom), pre-sell = kind 0 `dist_blackboxpark` (Black Box park) plus two challenges.

Change:
- skate-core: `extras` (rows as data, Back, select), `text_entry` (FE 46 OK / cancel rules and the on-screen keyboard
  state, layout as data), `cheats` (table as data, matcher, toggle state, dialogs); tests for each.
- skate-game: `extras_host.rs` (the natives, the cheat event `CheatChanged`), `text_entry_ui.rs` (the keyboard
  drawn as a Bevy UI panel while FE 46 is on top; hidden while a popup shows), `retail_menu_stage.rs` (FE 18 / 46
  open / close, Back, SetState(10) once loaded, the keyboard input: pad grid, A type, X delete, Y shift, Start done,
  B cancel; PC typing, Backspace, Enter, Escape), the cheat dialogs through the popup list, `living_world` applies
  the zombie toggle to `LivingWorldSettings.zombie` (session only, like retail per b304).
- Movie set version 9 (`main/extras` added; an older set forces Refresh Game Files). The asset check now checks the
  set the menus load (`SKATE3_MENU_MOVIES` when set).
- NOT RETAIL YET / ours: the keyboard look (since 2026-10-22 the Extras popup frame, see "The cheat entry in the menus' popup style"); who posts retail's first SetState(10); the zombie mode gate (retail's mode
  values 0 / 3 / 4 not decoded; ours: Free Play). Rows 1-3 (the code unlocks) unlock directly (`ExtrasRules::default`
  in `skate-core/src/extras.rs`); Credits / Licenses / Skate Reel stay greyed until FE 25 / FE 3 / FE 24 are ported
  (Licenses: the FE 3 viewer work is kept, WIP, user 2026-10-19). The other retail cheat codes change things the engine does not have and are not listed.

Verification: skate-core `extras`, `text_entry`, `cheats` tests (10); `extras_host` test;
`retail_extras_screen_lists_its_rows_and_opens_cheat_entry` (data-gated: the real movie lists the rows through our
natives and A on row 0 asks for the cheat entry); skate-core 1142, skate-game 819, skate-mods 106 pass. Not yet
looked at in the running game (keyboard look, popup over the keyboard).

## Cheat entry keeps the game paused; its keys are logged; --check-assets sees out-of-date menus (2026-10-19)
- **Report:** laptop 2026-10-19 16:11: "the cheat entry menu is unusable and doesn't keep the game paused." Opening
  FE 46 from Extras closed Extras (gameplay resumed) and FE 46 has no suspend flag in the retail screen table, so the
  game ran behind our keyboard. Retail's FE 46 hands over to the console's system keyboard, which takes the screen.
- **Change (ours):** `FeBackdrop::info` gives FE 46 (`47ddadb752e051b6`) `suspends_gameplay` (test
  `the_text_entry_screen_suspends_gameplay`). Every key the on-screen keyboard takes is logged (`TEXT_ENTRY input=...
  row col key text shift outcome`, `TEXT_ENTRY typed`), and a key that reaches FE 46 while the keyboard is not taking
  input warns, so the next session shows why typing did nothing (the code path looked correct on reading).
- **Asset check:** `--check-assets` (the launcher's prebuild check) now runs the same file check as the start-up
  `ASSET_CHECK`: `SKATE_ASSETS_STALE group=... reason=...` per finding, and `SKATE_ASSETS_REFRESH_NEEDED` with a failing
  exit when the launch would restart into Refresh Game Files (laptop 2026-10-19: it printed READY with v9 menus while
  the game reads v10, so a play session became a refresh).

## The cheat entry in the menus' popup style (2026-10-22, USER-ASKS 239)

- **Problem (the user):** the cheat entry screen needs a better UI and implementation. The keyboard was a generic Bevy panel (system font, flat dark box, button help as wrapping text) over the unblurred game, unlike every other menu screen.
- **Retail:** FE 46 opened the Xbox system keyboard (b305), so the look is ours. The user chose "Skate 3 popup style" (2026-10-22).
- **Change (`text_entry_ui.rs`, `retail_menu_stage.rs` render, `retail_menu_movies.rs` `shared_text_assets`, `fe_backdrop.rs`):**
  - The keyboard is now drawn on the menu stage's layer as stage draws, not a Bevy UI panel.
  - The frame is the Extras popup's own `controls/panel` frame rebuilt at the keyboard's size: 16 px paper corners, 8 px edges, cream inner paper, the title band, the body, the tinted inner corners and side strips, the paper grain at 0.325, the cream footer. Pieces, UV turns and colours are copied from the Extras movie's draws (`SKATE3_DUMP_DRAWS=1` on `retail_extras_screen_lists_its_rows_and_opens_cheat_entry` prints them, with the UVs of the panel pieces).
  - The title is in Futura Overprint (tan), the keys and the field in Futura Std Medium, and the highlighted key in the settings rows' selected blue (0x37A5FF).
  - The prompts (A Select, X Delete, Y Shift, Start Done, B Back) are the connected pad's glyphs (`prompt/<button>`, as the movies' CustomButtons) with futuraheavy labels, right-aligned on the paper footer like the Extras popup.
  - The game blurs behind FE 46 like behind the menus (ours; mods can turn it off per screen).
  - Input is unchanged: pad grid, A type, X delete, Y shift, Start done, B back; PC typing, Backspace, Enter, Escape. The PC key map only binds arrows, F8, Enter and Backspace, so typed letters and Space never double as pad keys.
- **Verification:** `keyboard_draws_frame_keys_and_prompts_on_the_stage` (frame pieces, every key, five prompts, all on the stage, one lit key, nothing while hidden or closed). Automated runs (`SKATE3_RETAIL_MENUS_KEYS`, pause menu > Options > Extras > Enter Cheat Code): the panel draws in the retail look; "zombie" typed on the pad grid and Start gives "Zombie Mode Enabled". Not yet seen by the user in play.
- **Open:** PC typing on the laptop (ask 110) is not confirmed: the `TEXT_ENTRY typed` lines will show it in the next session.
## Credits (FE 25): WIP, row greyed (2026-10-22)

- **Retail [code / data, research b489, template and screen record verified]:** template 30 `Credits` (`extras/credits.swf`, only in fedynamic.big), screen `0bdb6ab1cb803a1b` (suspends gameplay, no blur, no movie input). The lines come from `data\credits\credits.txt` in miscload.big (Latin-1, `<font> <size> <spacing> "<text>"`, `#ID_` ids localised, `\n` / `^` splits); the movie asks for them through `FECredits` (GetNumCredits, GetFontSize, GetLineSpacing, GetLineOffset, GetLine, ExitCredits), scrolls them itself and calls ExitCredits after the last; B pops the screen.
- **Done (uncommitted parts committed 2026-10-22):** skate-core `credits` (parser, Latin-1 / UTF-8 decode, the six answers, mod sections; tests), `extras_host::CreditsHost` / `CreditsMods`, the stage's FE 25 open / close / Back / Rolling / ExitCredits, `sdk.menus.credits(key, lines)` (skate-mods `MenuCredits`, cleared on mod disable), the exporter's narrow fedynamic cache for the movie and `credits.txt` in the set; menu movie set version 13.
- **Not working yet:** the screen opens and loads 979 lines, but no line draws (the movie's `createTextField` lines; next: dump the tree / draws after SetState(1)), and the Back label shows its raw tag `<B:B_hud>`: inline button tags in APT text are not laid out (research b491: parser 82CA14D0 turns each tag into `||`, drawn by 82808708 as a white icon, pen advance 48 x scale - 2; 194 strings use them). The Extras Credits row stays greyed until both work.
## SpawnSkater (FE 62)

2026-10-23. The Free Play pause rows Free Play (SoloFreeskateOptions) and Call Skater (SkateWith).

- **Retail [code / data, research b543 / b544 / b545 / b562; key and modes verified]:** FE 62 (info record 0x83026088 + 62 x 24, key `6abd6ed78ce7e627`, disc index 60; suspends gameplay, no blur, movie input) opens template 107 `SpawnSkater` (`cas/spawnskater.swf`). The crossbar sets FE 62 +0x40 before the push: 200 for Free Play, 100 for Call Skater (b307); the movie's prototype has `eScreenMode_SkateWith` 100, `_Freeskate` 200, `_DemoPublic` 300 and states `State_UpdateList` 1, `State_UpdateButtonHelp` 2 [data]. Host object `SkaterSpawner`: `GetSkatersData` returns `[rowCount, names[], isHeader[], spawned[]]` (one header per group, then its rows; raw names as `#name`), `GetScreenMode`, `GetLastMenuHighlightIndex`. Groups (b544): You (Free Play only), Teammates, Community (online), Pros, IP. A in Call Skater calls the skater into one of 3 companion slots (limit popup `ID_CALL_SKATER_LIMIT_REACHED` when full); A in Free Play makes it the player's skater (flow 0x89: fade, reload the player record, close FE 62; position, board and stats stay). B closes; X dismisses a called skater (Call Skater) or opens the Free Play options FE 34 (Free Play). Highlight previews the skater (flow 0x8D) outside Call Skater.
- **Ours:** `skate_core::spawn_skater` (modes, groups, rows, select / highlight / back / X, companion limit; rules are data), `spawn_skater_host.rs` (`SkaterSpawner` natives and the picker callbacks; groups from our character library `custom_models.rs`: You = the stock skater with the saved outfit (`request_stock` only, the customiser stays as it is; independent of upstream PR #67, which makes the PC overlay's Use stock skater take the outfit off), Pros = roster category Pro, IP = Special, then the player's own imported characters as an extra group `#Custom`, ours), the stage's FE 62 open / close / keys / actions (`retail_menu_stage.rs`). A Free Play pick swaps the character through `CustomModels::request_character` (the same path as the PC picker, so it is saved in `selection.json`) and closes FE 62 and the pause menu. Every action also goes out as `SpawnSkaterRequest` for the systems that own them and for mods. Menu movie set version 14 adds the movie.
- **Fade around the swap (2026-10-24, USER-ASKS 299):** A in Free Play holds the FE fader bit 7 (0x89 state 82706938 -> 825E6B30, about 0.42 s at the 60 Hz FE tick); at black the callback (82706AE8) requests the character from the library; when the swap is done (loaded, failed or timed out) FE 62 closes with the pause menu and the bit is released, so the game fades back in with the new skater (`retail_menu_stage.rs` `FADE_SWAP`, `swap_due` / `swap_loading`; `CustomModels::swapping`).
- **NOT RETAIL YET:** the pause menu closing with FE 62 (inferred: Free Play resumes); the select / X fe sound keys (0xF26173CBBC2075B4, 0x17472B6D55E36C95) are not named, the A sound stands in; the You row label; header rows not pickable (inferred); the first State_UpdateList is posted on load (its retail poster is not read); no Teammates or Community groups (no source in the engine); the preview is not ported; Call Skater lists only pros in the living world's skater pool (pool membership stands in for retail's attribute 0xCABC362B1B05CA19). Called skaters are served by the living world: docs/hails-additions/26b-living-world-population.md#called-skaters-fe-62-call-skater.
- **Preview placement (2026-10-24, USER-ASKS 326; recomp trace b585 run1 / run2, hooks
  `hooks_marquee.cpp`; research b575 to b580, b585):** the MarqueeViewer stages the previewed skater 200 m above the
  map: the character actor at (-1.0, 200.0, -7.05), heading -1.7453 rad (actor +0x30 / +0xB0, `82546978` /
  `82546EA0`), the same for each skater picked in the list [trace]. The camera is set once when the preview starts
  (`82530C68` from `825319A0`): eye (-4.0, 200.48, -7.4), forward (0.989, 0.127, -0.076), about 3 m in front of the
  skater and aimed at a point about 0.86 m above its feet [trace]. The entry's camera value (-3.8, 0.88, -7.6)
  (settings +0x20) is therefore a height on that stage, not a world Y: the earlier "character Y 200 vs camera Y 0.88"
  contradiction is the stage offset. The viewer camera functions b585 expected (`82546648` / `82546500`) did not run
  in this pass (no stick input); the stage backdrop is a concrete wall and ledge (screenshot `run1_shots`, 38.9 s).
  Open: the exact eye formula from the entry value (x differs by 0.2 m, y by 0.4 m), free-look under the sticks.
- **Tests:** `spawn_skater` (rows, limit, dismiss, Free Play pick / preview / options / close), `retail_spawn_skater_screen_lists_skaters_and_picks_one` (the movie loads, every call answered, A picks). Automated in-game runs (keys `_x60,13,_x30,13,_x30,2`: Call Skater with 33 rows; `_x60,13,_x30,2,_x120,13,_x30,2`: Free Play, picks Andrew Reynolds, the player swaps). Not play-tested by the user.

### Runtime pro swap T-pose

2026-10-25, USER-ASKS 343 / 344. Ours (engine bug, no retail question).

- **Problem:** a saved pro animates at startup, but swapping to a pro at runtime (FE 62 Free Play pick, or Extras > Custom Models) left the new pro in a T-pose. Dem Bones (same 35-bone rig) animated after the same swap. Laptop sessions game-20261025-115957 and -125257; reproduced on the desktop with `SKATE_CUSTOM_MODEL_SWAP=danny_way@15` (saved Andrew Reynolds).
- **Root cause:** the living world preload (`living_world/preload.rs`) loads every NPC skater look as `<pro>/character.glb#Scene0` and holds only that handle, so the bare GLB (`Gltf`) asset is dropped once it has loaded. `custom_models::publish` loaded the bare GLB again (for its load state) next to the scene. With the bare entry gone Bevy 0.18 loads the whole file again and re-inserts `Scene0` into its live slot, which sends `AssetEvent::Modified`; the scene spawner then despawns and respawns every instance of that scene. The swap had already bound the first instance's joints (its scene was loaded through the preload, so the instance was ready at once), so the visible respawned instance was unbound. Debug logging: bindings alive 0/35 after the swap, the visible meshes' joints new entities. Dem Bones is not in the NPC skater pool, so nothing preloads it and it loads in one go. The respawn would also hit an NPC skater wearing the same pro (inferred, not observed).
- **Change:** `publish` loads only the scene (`GltfAssetLabel::Scene(0)`), whose live handle never reloads the file, and reads a failed bare GLB through `AssetServer::get_path_id` without loading it (`custom_models.rs`, `Pending { path, scene }`).
- **Tests:** `custom_models::tests::swap_loads_the_scene_so_a_preloaded_instance_keeps_its_joints` (a scene-only preload keeps its joints when the scene is loaded again; loading the bare GLB respawns them, the hazard). Test hook `SKATE_CUSTOM_MODEL_SWAP=<id or roster key>@<seconds>` (or `stock@<seconds>`).

### FE 62 preview

**Problem.** FE 62 showed no preview of the highlighted skater (port step left open since 2026-10-23).

**Retail** (research b575..b580, placement traced b585): FlowMessage 0x8D starts the MarqueeViewer in Free Play and
Demo (not Call Skater): the player's character is unloaded, the highlighted one streams in and stands on a stage 200 m
above the map at (-1.0, 200.0, -7.05), heading -100 degrees, playing `MARQUEE_STAND` from
`sk83_marquee_viewer.abin` (loops by its own flag); the camera sits at (-4.0, 200.48, -7.4), aimed about 0.86 m above
the feet, 60 degree FOV. Leaving FE 62 shuts the viewer down and restores the player's character and the camera.
The stage is the Create-a-Skater set `DIST_CreateSakter` (miscload.big), modelled in place at x -7.4..0.4, y 200..215,
z -14.9..0.3 [data]: the concrete wall and ledge of the recomp's shots (b585 `run1_shots`).

**Change.** `marquee_preview.rs`: the menu stage sets `MarqueeView.open` (FE 62 on top, not Call Skater); each 0x8D
preview swaps the player's character through `CustomModels::request_preview` (a swap that does not become the saved
selection), the player rig plays the stand clip at the stage through the Trick Guide's clip pose (`SceneClip::
parse_named` reads a named clip from a bank), and the gameplay camera sits at the traced eye looking along the traced
forward (0.989, 0.127, -0.076) (b585 `MQSET`), which puts the skater right of centre as in retail. The first highlight
is posted in the frame FE 62 opens, before the stage reports the screen open, so it is kept until the viewer closes;
the rig is bound again when outfit parts spawn after a swap. Closing puts the player's own
character back unless FE 62 made a real pick. Setup's hud group exports the stage (`scene_clips.py`, its decal and
alpha-test meshes included) to `private/hud/scene/MarqueeStage.skate`; the viewer spawns it in world space, lit by the
map's environment, and despawns it on close. The clip's forward is +X, so the heading turns by a quarter turn on our
axes (side on otherwise; with it the vault's -100 degrees faces the traced eye within 4 degrees). The stage, camera,
bank and clip are data (`MarqueeStage`, retail defaults).

**Verification.** In game (automated keys pause > Free Play > one row down): Andrew Reynolds stands on the stage in
front of the concrete wall, facing the camera with his board; B brings the player's character back (`MARQUEE_VIEWER closed`, the
custom model reloads); the saved selection file is unchanged (hash). Tests `the_retail_stage_faces_the_character_to_
the_camera`, `marquee_stand_parses_from_the_stock_bank`.

**Camera input.** Retail's viewer has a camera input handler (b579 / b580), but it runs only while the viewer's record pointer (+0x60) is clear and FE 62 keeps it set (b578); the user confirmed the sticks do not move the FE 62 preview camera (2026-10-29), so the port has none.

**Swap frames (2026-10-29).** Scrolling the list showed the map for a frame now and then: while the next character's model
swapped in, `drive` returned before it placed the stage camera, so the gameplay camera's frame was drawn. The stage camera
is now set on every open frame (`stage_camera`), the swap frames included.

**Open.** A fade around the swap; a mod API for `MarqueeStage`.

## Free Play options (FE 34)

2026-10-23. X in FE 62's Free Play list (flow 0x8A).

- **Retail [code / data, research b548 / b564; natives and labels verified in the movie]:** FE 34 (key `898675eacd78bf05`, disc index 32) opens template 13 `FreeSkateOption` (`main/freeskate_options.swf`). Host object `FreeSkateOption`: `GetSelection` (row), `GetTrafficSetting` / `GetPedestrianSetting` (tenths, `trunc(v x 10 + 0.1)`, 0 below 1), `GetAISkatersSetting`; the movie's SetState 0 / 1 / 2 refresh a row (its slider sound is the movie's own `Audio.PlaySound`), 999 marks the selected row; its UpdateInput is empty. C++ input 8260A4E8: Up / Down move the row with wrap (`core_nav_up` / `core_nav_down`, SetState 999), Left / Right step 0.1 (top 1.0, below 0.09 snaps to 0) or toggle A.I. Skaters (SetState row), B writes the three values back (0x830B7C34 / C38 / C3C), plays `core_b_button`, posts flow 0x8B and closes; no cancel. Init copies the saved values into the screen.
- **Ours:** `skate_core::free_skate_options` (rows, steps, snap, write-back; rules are data), `free_skate_options_host.rs` (the four natives), the stage's FE 34 open / keys / close; B writes `LivingWorldSettings.free_play`, which the living world already applies (b551: census traffic and ped density, A.I. skater spawns).
- **NOT RETAIL YET:** the first refresh states (0, 1, 2, 999) are posted on load (their retail poster is not read); the movie's `Audio.PlaySound` slider sounds are not played by our stage (unverified); FE 62 is reopened after B (the stack's reopen).
- **Tests:** `free_skate_options` (band edges, row wrap, steps, clamp, write-back). Automated run (keys `_x60,13,_x30,2,_x120,5,_x120,14,_x30,14`): FE 34 opens from FE 62, Traffic shows 8 of 10 bars after two Left. Not play-tested by the user.
## HUD minimap

2026-10-23 (user: build it for the Call Skater icon, and wire it to the retail Minimap setting).

- **Retail [code / data / video, research b565 / b568 / b569; template, texture and video frames verified]:** a HUD movie `hud2/hudminimap.swf` (template 53 MiniMap) draws the frame: a 128 px round dark glass disc at (1063, 46) with a tick ring; the C++ HUD component draws a north needle over it (82665848, `north_pointer` of `icons/minimap.rx2`, 16 x 128), turned by the smoothed heading (82665388: above 1 m/s `normalize(0.85 x old + 0.15 x travel direction)` per update, else the skater's facing; heading-up), then the icon groups of `Sk8ChallengeSystem::MiniMapManager` (8268CB40: size = item scale x 32 px, alpha 1 to 71.28, `1 - (d - 60) / 290` beyond, 0 past the row's max distance, default 1000). There is NO map picture: the `MiniMapTexture` resource type and the `minimap` material exist in the code, but no shipped data uses them (b569), and the retail video (2026-10-21 08-45-27.mp4, 0:40 and 1:30) shows the empty glass disc. Shown (82665278): never with the HUD off; game modes 1 / 6 on, 2 off, else the Game Settings Minimap row (settings +158, default on).
- **Ours:** `skate_core::minimap` (visibility rule, heading smoothing; tests), `hud_minimap.rs` (the movie on its own VM, Show on changes; `MinimapHeading` stepped at the console's 30 updates a second from the player's motion), the scoring HUD draws it (`scoring_hud.rs`, own draw layer, the needle quad from the menu set's `texture/north_pointer`); the Minimap settings row is no longer greyed and decides it (`retail_menus.rs` `NO_EFFECT_YET`). Setup: `prepare_hud.py` exports the movie (with placement clip events, as the menu movies), `install_prepared_hud.py` installs it as optional; a minimap that does not load leaves the rest of the HUD running.
- **Needle and heading (b571, constants verified):** north is world -z (82665848 measures the heading against (0, 0, -1): 0x82165A10 = 0, 0x8216DEE0 = -1; our importers keep retail's axes); the needle eases once more towards the heading (0.85 / 0.15, 0x822250F8 / 0x820994B4) and is the pointer quad of +-16 x +-100 map units (0x8232F9C0..F9FF), 16 x 100 px at the 0.5 scale. The heading eases towards the travel direction above 1 m/s (normalised) and towards the camera's forward below it (not normalised); retail freezes it under a skater state bit (R+0x94 0x20) we do not have.
- **Icon groups (2026-10-24, USER-ASKS 299; research b574; 8254E2E8 and the fe_map_icons values verified):** retail's MiniMapManager has five groups, drawn in the order they were added (82644BB0 appends). Mode 4 (vt+0x0C 8254E2E8) adds the challenge group (challenges in the current location, minus types 4 / 9 / 0x2A / 0x33, the challenge's icon field 0xE616ACDF5A36BFAC, rebuilt every update by 8254DF70) and the skater group; mode 8 adds only the skater group with show-all set (every AI skater, not only called ones); any other mode adds only the skater group. A running challenge adds its own group (8254DB30, removed by 8254E258), filled by the challenge scripts (Lua 82849B78: icon by name, default `race_gate`; 82849DE8: the friend-film `mini_teleport`); challenge discovery adds one in mode 4 (8255B740); a message-driven group (82577338, handler 82577560) shows / hides the movie and adds / removes icons by name. Teleporters, pools, properties and shops (fe_locations) are on the pause Challenge Map only, not the HUD minimap. Max distances are fe_map_icons data (teleport and spot 500, property 400, pools 300, else 1000).
- **Ours:** groups keep retail's add order (`MinimapIcons::set` appends a new group, keeps an existing one's place). fe_map_icons records are read from the vault (`skate_data::challenge_map::map_icons`, a name also matches its hash64 key as the scripts' lookup). Mods get retail's script path as `sdk.minimap.icon(key, {position, icon="race_gate", ...})` / `sdk.minimap.remove(key)` (`modding/minimap.rs`, sdk/ENGINE_API.md "Minimap icons"): one group per mod, 32 icons per mod, gone when the mod is disabled. The message-driven group's show / hide (msg 0 / 1 set the `minimap` element of movie `main` visible) is `MinimapIcons::set_hidden` / `element_shown`, ANDed with the visibility rule (2026-10-24, USER-ASKS 301; [inferred] the element flag and 82665278's decision both have to allow the draw); mods reach it as `sdk.minimap.show(false / true)`, one hide per mod, gone with the mod.
- **Minimap mode (2026-10-24, USER-ASKS 326; research b585 (A), the
  chooser re-read) [code]:** `82551E58` asks `82552750` for the mode and, when it changes, leaves and re-enters
  every challenge-system manager with it (`82552650` -> vt+0x0C; the MiniMapManager's is `8254E2E8`). `82552750`:
  online (the Simulation's vt+0 `8285A870`, byte 0x830B7C2B) -> 8 (show-all); else GameMode (0x830B7AE8) 2 -> 0x10;
  else `[0x830670CC]` == 2 -> 1; else GameMode 0, 3 or 7 -> 2; else (1 career, 4, 5, 6) -> 4. So offline Free Play
  (FE 62's skater pick sets GameMode 3, `82706B40`) is mode 2: the called skaters' group only, which is what ours
  shows; career (mode 4) adds the challenge group; show-all is online only (networking is not ported). Nothing to
  port offline. Confirmed by the hook run b585 run1 (`MMMODE`): mode 1 while the game boots, then 4 when the career save's gameplay starts (MiniMapManager left and re-entered) [trace]. Open: what `[0x830670CC]` == 2 is (mode 1, seen during the boot).
- **NOT RETAIL YET:** the challenge, running-challenge and discovery groups (our game has no challenge system yet; the
  career mode would show them); the message-driven group's senders and message ids (0x830B7E78 is filled at run
  time).
- **Map matrix and rim (recomp trace 2026-10-23, a local trace, hooks MMDRAW / MMQUAD in the recomp's `hooks_hud.cpp`):** the minimap draw 82665388 gets a constant matrix: 0.6291 px per map unit on x, 0.6293 on y, centre (1128, 110) on the 1280 x 720 HUD (88 samples, one value all run); map units are metres. Far icons are held on a rim of 71.281 units (760 of 984 icon quads had their centre at exactly 71.28, none beyond; 0x821E956C), 44.8 px. Icons are 32 units (20 px), the needle 32 x 200 units (20 x 126 px). These replace the video estimates (0.5, 45 px) and are the `IconRules` defaults.
- **Icons (b570, constants verified):** each icon is placed heading-up around the player (`skate_core::minimap::place`: `R(yaw) x (R(angle) x corner x 32 x scale + (icon - player))`, then the scale; the fade 1 up to 71.281 m, `1 - (d - 60) / 290`, 0 past the max distance, default 1000; turned with the map plus its own angle; tint per icon). Groups are data (`hud_minimap::MinimapIcons`, drawn in order, the needle last); the called skaters' group puts `mini_skater` on each Call Skater companion. Automated run: Atiba Jefferson called, his icon on the rim.
- **Check:** automated Downtown run with the FPS counter off: the disc and the turned needle draw at the retail spot. Not play-tested by the user.
## Challenge Map: tab rules, row counts and Debug Menus (2026-10-21)

- **Problems [screenshots, automated with `SKATE3_RETAIL_MENUS_KEYS`]:** next to retail's list (recomp, Locations tab)
  ours had no row counts ("Downtown District (14)" in retail) and a fourth tab "debug_list_all" retail never shows.
- **Tab rules [data]:** each `map_listing` carries a tab Lua (`Hash_3E294F92C056A144`, skatercollections): Challenges
  (`progression_active`) and All (`progression_all`) hide in free skate, Completed also while nothing is completed;
  Locations always shows. Ported as menu rules (`menus.rs` `install_retail_rules`, facts `game_mode` (missing = 0,
  free skate) and `completed_challenges`). Our pause menu is retail's Free Play one, so the map shows Locations only.
- **Debug listings [code + data, research b479, verified the hash and the
  values]:** `Hash_755772463A459824` is `IsDebug` (hash64 of the name); only `progression_all` has it set
  (`default` = 0). Retail's filter 82D0C0E0 skips an IsDebug row when filling the tab list (82D0BDD0) and the folders
  (82D0D728); nothing in TU3 turns it on. Setup's vault reader exports the flagged listings
  (`skate_data::challenge_map::debug_listings`, `MenuData::map_debug`) and the registry hides each unless the fact
  `debug_menus` is set, so mod listings with the flag behave the same.
- **Debug Menus (ours, not retail):** PC overlay EXTRAS > Debug > "Debug Menus" (saved in `settings/graphics.json`
  `debug_menus`) sets the fact. With it on the map gets the "debug_list_all" tab: the challenge-type folders (empty,
  our game has no challenges yet), Art Gallery, Maloof Money Cup 2010 NYC and a blank row: the folder
  `progression_all_team_spacer` points at a map_category with an empty label (a spacer); folders that point at a
  category now take its label, and an empty label shows no count.
- **Compiled out in TU3 (b479):** the pause / start hub "Debug Menu" (`ID_PAUSEMENU_DEBUGMENU`,
  `ID_START_HUB_DEBUG_MENU`: only the strings and a `debug` icon frame in core_menu remain, no table row or screen)
  and an "AI Paths | Options" page (strings only). Nothing to re-enable; anything there would be ours. Challenges
  flagged `DebugOnly` (0xFD40E4A31027C984) are kept out of the map's item pool by 82645B70; no exported challenge sets
  it.
- **Row counts [code, b255]:** `sub_8264ACF8` localizes a row's label and formats "%s (%d)" when the child's showCount
  is set; ours: folder rows (`map_listing`) carry their real row count (`challenge_map::Row::count`, headers left
  out) and the host appends it after localizing. Capture: Downtown 14, Industrial 12, University 10, skate.School 1,
  skate.Park 8, Maloof 2, Black Box 1, MMC NYC 1, Danny Way 1, Sanatorium 1, Art Gallery 1, the same as retail.
- **Tests:** `challenge_map` (tabs in career and free skate, folder counts), `menus` (IsDebug listings hidden without
  the fact), overlay rows. Not play-tested by the user.

## Code reference: apt_stage.rs

Detail moved out of the code comments of `crates/skate-game/src/apt_stage.rs`; each comment there links its section here.

### Front-end stage

The front-end stage (doc 31, Milestone 5a): several retail menu movies running in ONE script VM, like retail's single APT player where FE_root, the screen manager and every screen movie share `_global`. Movies are loaded into clips at runtime (MovieClipLoader.loadClip, used by screenmanager.apt OpenScreen) and clips are created by script (createEmptyMovieClip).

The C++ side of retail (screen stack, Game.OnScreen* handlers, CoreMenu, ...) is supplied by a `StageNatives` implementation: the stage only routes calls on the named host objects to it. Movie sources come from a `MovieSource` (the linked retail set with mod overrides layered in, `RetailMenuMovies`), so a mod-replaced screen is what gets loaded.

Each loaded movie keeps its own timeline rate (`Movie::milliseconds_per_frame`), so screens run at their authored speed for any engine frame rate.

### Loaded root clips hold

Retail load completion (sub_82E7EF28) gives a loaded root clip flag 0x80, not the playing flag 0x40 (set only by play(), action 0x06 82E6BCC8); the tick (82E5B640) advances 0x80 clips only when the APT config byte [0x83037510+64] is set, which Skate 3's init leaves 0. So a loaded root runs its frame-0 actions once and stays there until its script calls play(); its child sprites play as usual.

### Stage object collection

The stage frees objects nothing reaches any more, between updates: closed screens leave their clips, listeners and script objects behind, and without this a few pause menu opens filled the VM's object budget. Roots are `_global` plus every object id the stage and its movies hold. The script-made clip table is weak: a clip made inside a screen must not keep that screen alive through its parent once the screen unloads, so its entry goes when the clip is collected. NOT RETAIL (mechanism): retail's APT player refcounts its objects; this only keeps the slots bounded.

### Stage draw order

Draws every level under the clip it was loaded into, in display-tree order (`display_path`: the screen manager's screens under gScreens, popups under gPopups above them), load order between equal places. `scene` gives a level's shapes and textures. NOT RETAIL YET (order): retail draws one display tree by depth; here each level draws as one block at its holder clip's place (a level's clips never interleave with another level's).

## Code reference: challenge_map.rs

Detail moved out of the code comments of `crates/skate-data/src/challenge_map.rs`; each comment there links its section here.

### Challenge Map tree

The retail Challenge Map tree (FE screen 17) from the vault collections (skatercollections), as menu map nodes.

[data] (research b244 / b255): `map_listing` rows form the tree by parent (the root is the mode key, `progression` for career and free skate, `online` online; its children are the tabs `progression_locations` / `_active` / `_completed` / `_all`). Each row names its folder's filter (`Hash_E0762EA3F90C9EEB`, a RefSpec of class + key, a `map_filter` or a `map_category`) and a sort order (`Hash_982BD1EAE2334504`, u16 in the high half). Retail fills a folder by running its filter's Lua over the item classes; the location filters (`downtown` ...) keep the `fe_locations` rows whose World (`Hash_7565CD2A80E93A5E`, inherited) is the filter's world (`LocationFilterHelpers.IsLocationInWorld(item, "dist_downtown")`; research b447: the disc Maloof filter names `dist_maloofmoneycup`, its rows' World, not their District `dist_maloof`; the DLC folders do the same). Those are ported as data ([`LocationRules`], retail defaults incl. the DLC packs' folders, mod-overridable). The skate.Park folder's filter (`skatepark`) lists `world` rows with `IsSkatePark` and `AllowInSkateParksTab` (b447), as`World` leaves labelled by the world's own text. Other filters (challenges) are not ported yet, so their folders have no leaves and stay greyed. Retail's unlock test (`Unlock.IsLocationUnlocked`, b450) is ported over [`LocationUnlocks`] (no unlock store yet).

### Location unlocks

Which locations count as unlocked for the folders' unlock test (b450 / b458): retail lists a location row when its `Hash_90E678B6961730E1` flag is set, else when the unlock manager holds the row's name (or `all_locations_unlock`) in either list: the saved profile list (a fresh save holds no locations, `825046B0`) or the list rebuilt from the player's DLC `data\unlocks\*.unlock` lines (`82505220`; e.g. Time is Money's `LOCATIONS,dist_maloof_street`). `granted` = the names a DLC line (or a mod) unlocks. Until the profile unlock store exists, everything else counts as unlocked except `locked`: the two start rows no grant path names (`gamestart`, `dist_skateschool`; b450 / b458), which matches retail video. NOT RETAIL YET: retail's saved list (the Lua / challenge rewards that grant locations are not traced).

## Code reference: retail_menu_stage.rs

Detail moved out of the code comments of `crates/skate-game/src/retail_menu_stage.rs`; each comment there links its section here.

### Retail menu stage dev view

Dev view of the retail pause menu on the front-end stage (doc 31, Milestone 6).

On unless `SKATE3_RETAIL_MENUS=0` (`SKATE3_RETAIL_MENUS_OPEN=1` also opens it after 4 s). Start and F8 open / close the retail pause menu (core_menu through the retail screen manager, retail CrossBar arguments); while it is open the arrow keys, Enter and Backspace drive the ported crossbar handler (skate_core::crossbar), whose sounds play as fe records and whose SetState goes to the movie through `_global.ScreenManager.SetScreenState` like retail's C++. The PC overlay (`graphics_menu`) stays on Guide / F1 / Escape, and takes Start too while the retail pause menu cannot open (`RetailPause`).

Input: the pad (all four controllers) and the keyboard (arrows = d-pad, Enter = A, Backspace = B, F8 = Start) feed retail's Apt actions (`skate_core::fe_input`); `SKATE3_RETAIL_MENUS_KEYS=13,13,2` injects key indices once the menu is ready (screenshot checks). Repeat: retail's tick rule (fe_input; tick rate assumed).

### Replay screen choice

The instant replay screen and its help (FE 12 / FE 13; b391): FE 12's init (826102A8) opens the Create pack's editor (TU3 template 71 Replay3, `replay2/replay_editor3.swf`, help 74 ReplayHelp2) when product CCF0007 is owned, else template 72 Replay2 with help 73 ReplayHelp (b410 / b411). On PC the pack counts as owned (look rows always on): the first screen whose movies were exported wins. Opened by the engine with the replay, not by a menu push. `channels`: the screen shows the edit channels (look rows). Data: a mod may add its own screen first.

### Reopen and close pair

One stack batch can open a screen and close it again: op 5 closing a screen below the top unwinds to it and reopens it first, so the Challenge Map teleport's close of [map, pause] reopens the pause menu and closes it. The pair is dropped (the screen ends closed either way): its movie would load after the script's CloseScreen, which ignores a screen still loading, and stay loaded for good (repeated teleports filled the VM's object budget). NOT RETAIL (mechanism): same outcome; the other effects keep their order.

### ScreenManagerReady registrations

Retail ScreenManagerReady handler (825D9450), in its order, with values as "%d" strings like retail: templates (only CrossBar so far: NOT RETAIL YET, retail registers all 133 in table order), the 56 Apt keys (value = key index, the same index UpdateInput passes), FE_SOUND_<NAME> = "<name>" per fe record, then platform "0", screen size 1280 x 720, edge offset "136", PAL mode "false". The 191 challenge enums (eChallengeTypes ..) and RefreshSubtitle are not registered yet (no menu reads them so far).

### Replay timeline strip

The replay editor's timeline strip (retail 82509B80 -> 82508E08 bands, 82509298 keys; b403), in the strip clip's space (x = fraction x 817): one band from y 3 to 30 (the band bottom is the clip height, [guess]) coloured by the clip's camera type, brighter while "selected" (no key under the playhead, not instant mode); one 13 x 51 key quad per key after the first, `key_glow` under the playhead (within 0.00889), `key_default` else; in the camera edit modes the key under the playhead is not drawn. Values are retail's (data, `TimelineLook`).

## Code reference: extras.rs

Detail moved out of the code comments of `crates/skate-core/src/extras.rs`; each comment there links its section here.

### Extras rows

Free Play pause row Extras (doc 31 "Extras and cheat codes"): retail FE screen 18 (vtable 0x82305390, template 15 Extras, `main/extras.swf`, research b320). The movie lists the rows through Game.Extras_GetNumOptions / _GetOptionTitle / _MenuTitle and asks Game.OnMenuGetItemEnabled / OnMenuSelect; the C++ side keeps the list mode (main rows or the Skate Reel sub-list, +0x84):
- Back (input 0x82655658, never consumed): in the sub-list plays core_b_button, goes back to the main rows and posts SetState(10); in the main rows plays core_b_button and pops the screen;
- select (0x826558E8): row 0 opens text entry FE 46 for a cheat code, rows 1-3 redeem console codes, row 4 pushes Credits FE 25, row 5 shows the licenses in the text viewer FE 3, row 6 opens the Skate Reel sub-list.

What the console-only rows do on PC is ours: rows 1-3 unlock their content directly, Licenses shows this project's licenses, Credits (retail FE 25, b489) is WIP and greyed. Rows are data (`ExtrasRules`), retail's the defaults; a mod adds or hides rows.

### Unlock list entry

One entry of the profile unlock list (retail `sub_82503C30(list, kind, fnv1(name), ..)`, research b321; the hash is FNV-1, multiply then xor, b458, not FNV-1a as b321 said): kind `entitlement` (retail 8: a redeemed code), `sponsor` (6: a clothing / sponsor item, `cac_unlock_sponsors`), `location` (0: a world location), `challenge` (passed to TheSim+0xE0 vt+0x14). Retail kinds (b458, string table 8302CC7C): 0 LOCATIONS, 1 CHARACTERS, 2 REPLAY, 3 EGGS, 4 GESTURES, 5 CLOTHING_UNLOCK, 6 CLOTHING_SPONSOR, 7 SKATEPARK_SET, 8 ENTITLEMENTS, 9 WORLDS, 10 CHALLENGES, 11 MOVIE, 12 DLC_PRODUCT.

## Code reference: trick_guide_scene.rs

Detail moved out of the code comments of `crates/skate-game/src/trick_guide_scene.rs`; each comment there links its section here.

### Trick Guide demo code

The Trick Guide's 3D demo behind FE 14 (TU3: screen init 0x82618A40 loads DIST_TrickGuide, update sub_82619A20 posts "play <clip> with camera track <n>" to the trickbook object, which runs the ViewScene scene: one character "current_player" playing `scene/trickguide/<name>1.abin`). The player's own rig plays the clip (ViewScene's character is the current player), the set is spawned as a child of the player root (clip and set share the scene space), and the gameplay camera draws only the demo layer while the screen is open. Research: local notes, doc 31 "Trick Guide demo". The camera plays the entry's ICE ViewScene track (scene.big `ice_000.bin`, skate_core::camera_track), fitted to the clip's length (retail writes the clip length into the track), and each point is placed by its segment's SPACE / AVATAR (`place`): tracks 0, 1, 18, 20 follow the skater's root at height 0, track 3 keeps its eye fixed in the set and aims at the root, track 19 is fixed. DUTCH is in turns; FOV = 2*atan(15.96/lens). Measured in the recomp: a local research note, sections 7 and 8. The set takes render location `trick_guide`'s fog and sun (fog_trickguide; retail_sky::SkyPackage::load_scene). NOT RETAIL YET: csm_trickguide shadows; the rest of the location (exposure, post_fx) stays the map's; the lens angle taken as the vertical field of view; the followed root taken as the clip's bone 0.

### Trick Guide track point

A track point in scene space (sub_828F36B0). Track space is Z-up and maps to the scene as (y, z, x) (sub_82673560 returns game positions as (z, x, y)). SPACE 2 adds the followed object's position without rotation: AVATAR 5 = the scene character's root at height 0, AVATAR 7 = the same root at full height (sub_828278A8: the clip entry's transform; the demo has one character, the skater). Other SPACE values have no case and leave the point fixed in the set.

## Code reference: prompt_glyphs.rs

Detail moved out of the code comments of `crates/skate-game/src/prompt_glyphs.rs`; each comment there links its section here.

### Prompt glyphs

Button prompt glyphs for the connected pad (doc 31, "Button prompts"): the retail menus draw a native "CustomButton" component for the Xbox 360 button named by `_test`; we draw the same button as the connected pad prints it. Every retail button name has an equivalent on every family (retail's name list, TU3 0x82204614: Back, Start, R_Trigger, L_Trigger, L_Bumper, R_Bumper, L3_Click, R3_Click, Stick_Right(_Up/_Down/_L/_R), DPad(_Up/_Down/_Left/_Right), Stick_Left(...), plus A / B / X / Y):
- face buttons: SDL's label for the pad type (`ControllerKind::face_labels`): A / B / X / Y, Cross / Circle / Square / Triangle, Nintendo letters;
- shoulders, triggers, Start, Back, stick clicks: the family's printed name (LB / L1 / L, LT / L2 / ZL, START / MENU / OPTIONS / +, BACK / VIEW / SELECT / SHARE / CREATE / -, LS / L3);
- sticks and the d-pad: a stick or a cross with the direction marked (the same on every family). The glyphs are generated in retail's prompt style (a coloured ball or a dark pill, a light symbol, a dark outline), not console art; mods replace them through the menu bitmap path later. Size: retail's glyph spans about 0.83 of the CustomButton placeholder box (10 of 480 px in a recomp shot of the Trick Guide help row, local screenshots; the box is 32 of 1280), centred in it; placement is the movie's own (button_item2 lays the box out).

## Code reference: cheats.rs

Detail moved out of the code comments of `crates/skate-core/src/cheats.rs`; each comment there links its section here.

### Cheat codes

Cheat codes (doc 31 "Extras and cheat codes", research b304): the text from the Extras cheat row's entry goes to the matcher `sub_82656080`, which walks the cheat table (0x83025F00, `{code, id, title, enabled text, disabled text}`) with a plain case-sensitive strcmp (`sub_82AE89B0`) over the first 32 characters (callback 0x82655F98).
- no match: dialog ID_VALIDATE_CODE_TITLE / ID_UNLOCK_CODE_FAILED / ID_COMMON_CONTINUE;
- a mode-gated code (zombie, id 5) outside its modes: ID_INVALID_CODE_TITLE / the row's refusal text / ID_COMMON_OK, nothing changes;
- else the cheat TOGGLES (entering it again turns it off) and the dialog is the row's title with its enabled or disabled text / ID_COMMON_OK.

The table is data (`CheatRules`), retail's rows as defaults, so a mod can add codes (its own ids) and the engine keeps the on / off state ([`CheatState`]) as host state the cheat event changes.

## Code reference: camera_track.rs

Detail moved out of the code comments of `crates/skate-core/src/camera_track.rs`; each comment there links its section here.

### ICE camera tracks

ICE camera tracks (the cutscene Scene system's cameras, e.g. the Trick Guide demo's 21 ViewScene tracks in scene.big `data/scene/ice_000.bin`; parsed by `skate_data::ice_camera`). A track keys the camera elements of key group 0: EYE and LOOK (scene space, Z-up), DUTCH (roll), TANGENT_EYE / TANGENT_LOOK and LENS_LENGTH (mm).

Sampling ports the TU3 segment setup sub_828F0BB0 and the tangent sub_828F0990: each element is a cubic Hermite per segment, `((a*u + b)*u + c)*u + d` with u the segment fraction, a = m0 + m1 - 2D, b = 3D - m1 - 2*m0, c = m0, d = p0, D = p1 - p0. The tangent at a segment end is the non-uniform Catmull-Rom tangent in the segment's own parameter, `D*dt_n/(dt+dt_n) + D_n*(dt/dt_n)*dt/(dt+dt_n)` with the neighbour segment n (previous for m0, next for m1; none at the track ends: D), scaled by the element's TANGENT value at that key. Each element names its spline switch and tangent element (descriptor +60 / +64, read live in the recomp): EYE, DUTCH and LENS_LENGTH use CUBIC_EYE (87) / TANGENT_EYE (7), LOOK uses CUBIC_LOOK (88) / TANGENT_LOOK (8). A switch is a per-segment value; when it is 0 the tangents are D (linear). Each segment also carries the eye and look space (SPACE_EYE / SPACE_LOOK, 89 / 90) and the followed object (AVATAR_EYE / AVATAR_LOOK, 91 / 92), applied by the scene player (sub_828F2C38 -> sub_828F36B0). Data-driven: tracks come from the disc; a mod can replace or add tracks by index, segment settings included.

## Code reference: text_entry.rs

Detail moved out of the code comments of `crates/skate-core/src/text_entry.rs`; each comment there links its section here.

### Text entry

Text entry (doc 31 "Extras and cheat codes"): retail FE screen 46 (vtable 0x82304CD8, research b305) and the on-screen keyboard that stands in for the Xbox system keyboard it opens.

Retail FE 46 has no movie: init sets "show keyboard", the update calls `XamShowKeyboardUI` with the title, the default text and max_len + 1, and the result goes through the OK path 0x82639890:
- empty text and `allow_empty` off: the keyboard opens again;
- in modes 0 and 1 every character must be in FELanguage `ALL_VALID_ALPHANUMERIC_CHARACTERS`, else the warning popup (FE popup record 0x107: ID_COMMON_WARNING_TITLE / ID_VIRTUALKEYBOARD_ALPHA_NUMERIC_STRING_ERROR / ID_COMMON_OK) and the keyboard opens again after OK;
- valid: the caller's callback gets the text as typed (no case change), and FE 46 pops;
- cancel pops FE 46 (the cheat row passes no cancel callback).

The keyboard itself (`Keyboard`: a letter grid for the pad, typing for a PC keyboard) is ours, an on-screen keyboard; the layout and keys are data (`KeyboardLayout`).

## Code reference: ice_camera.rs

Detail moved out of the code comments of `crates/skate-data/src/ice_camera.rs`; each comment there links its section here.

### ICE camera file

ICE camera tracks from the player's disc (scene.big `data/scene/ice_%03d.bin`, loaded by TU3 sub_82827EB0; doc 31 "Trick Guide camera"). The file is little-endian (sub_828EDD80 swaps it in place on the console). A scene block is `{u32 1, u32 size, u32 scene hash, u32 track count}` followed by the track records back to back (sub_828F51E8). A record: +12 f32 length (s), +20 37 key groups `{u16 segments, u16 keys}`, then from +168 `sum(max(segments-2, 0))` u16, aligned to 2, `sum(max(segments-1, 0))` u16 interior key times, aligned to 4, then every element's values in element id order (sub_828EE070): element `id` of group g holds `segments` values (per-segment elements) or `keys` values, each a MSB-first bit stream padded to `((bits*count+31)>>3) & !3` bytes (32-bit elements are raw f32). The element table is `ELEMENTS` (registered by sub_82903840). Read here: group 0's keyed camera elements EYE_X..LOOK_Z (f32), DUTCH, TANGENT_EYE, TANGENT_LOOK, LENS_LENGTH, and its per-segment CUBIC_EYE / CUBIC_LOOK, SPACE_EYE / SPACE_LOOK, AVATAR_EYE / AVATAR_LOOK (values checked against the recomp's per-track player words for tracks 0, 1, 3, 18, 19 and 20). NOT RETAIL YET (inferred from the data): interior key time = u16/65535 * length and the dequantisation `min + raw*(max-min)/2^bits`.

## Code reference: animation/raw.rs

Detail moved out of the code comments of `crates/skate-core/src/animation/raw.rs`; each comment there links its section here.

### RAW clip codec

Stock RAW ("DR", codec word 0x00004452) SQT decoding: the uncompressed packed frames the scene clips use (`scene/trickguide/*.abin`, the Trick Guide demo). TU3 sampler sub_82D1E658 (VMX job after the VBR job 82D1DE08). The frame layout matches the pinned skate3_anim importer (tools/vendor/skate3_anim); its packed rotation does not (see `unpack_rotation`). Compression header: six channel-mask words (scale, rotation, translation; two words each), u16 frame size, u8 bone count - 1, u8 packed translation, u8 packed rotation. Every frame is `frame size` bytes from the data start.

### RAW packed rotation

32-bit packed rotation, TU3 RAW sampler sub_82D1E658: an axis in spherical angles times the vector length, w = sqrt(1 - r^2) (never negative). Azimuth A = bits 21..31 * 2pi/2047 - pi, latitude M = bits 11..20 * pi/1023 - pi/2, r = bits 0..10 / 2047. Verified: the header reads, the pi constant 0x82060C44 and the reciprocal square root; the cos/sin order of A and the signs come from the data (a symmetric T-pose, a local research note). No normalisation here (the engine normalises after nlerp).

## Code reference: hud_central.rs

Detail moved out of the code comments of `crates/skate-game/src/hud_central.rs`; each comment there links its section here.

### Central message HUD

The central message HUD: retail's `hud2/hudcentral` movie (centre-screen lines such as the chase's "ZAP!" / "BAM!"), run on the same APT VM as the scoring HUD.

Retail posts a message through `82663F38` (from the chase manager `826B4FC0` and other callers): it stores the formatted, localised text, a style (`+0x34`, 0 here) and an audio hook (`+0x280`, 0 = the movie's `kAUDIOHOOK_DEFAULT`), then sends the screen event 4. The movie's `SetState(4)` calls `ShowMessage` with `HUDComponents.Central_GetMessageData()` = `[text, style]` (style 0 off-white, 1 red, other = the celebratory banner) and `Central_GetAudioHook()`, which plays `FE_SOUND_CENTRAL_MESSAGE_DEFAULT` and the clip's intro. The getters' bodies are not decoded; their values are what `82663F38` stores [inferred from the stores].

Engine-facing: [`CentralMessage`]. Mod-facing: `sdk.ui.central_message(text)` (the same message).

## Code reference: fe_stack.rs

Detail moved out of the code comments of `crates/skate-core/src/fe_stack.rs`; each comment there links its section here.

### FE screen stack

The front-end screen stack (doc 31, "Screen stack"): which FE screens are open, in which order, and what opening or closing one does to the game. A port of retail's stack (TU3 recomp addresses; a local research note, push / queue code verified).

Two stacks: the main one (A, stack+16, queue +32) and the overlay one (B, +80, queue +96; the tips loader and such, no blur, no input). Requests queue up and run in a batch on the next update (sub_82D0A560): the main queue first, the overlay queue only when the main one ran nothing. A queue waits while its stack has more than one screen and the top one is still loading or closing. The host owns the movies: this returns `Effect`s (open / close / show a screen, gameplay suspend, chyron, HUD mode, focus) and is told when a screen's movie is ready. Screens are indices into the per-build screen table (ids differ between builds); records come from the setup-extracted table and mods override them by screen id.

## Code reference: video_calibration.rs

Detail moved out of the code comments of `crates/skate-core/src/video_calibration.rs`; each comment there links its section here.

### Video calibration

Game Settings > Video > Video Calibration (doc 31 "Video Calibration"): retail FE screen 51 (vtable 0x82303548, template 91 VideoCalibration, `options/videocal`). A guide for setting up the TV in pages (settings, contrast, brightness); it changes no game value and saves nothing. The screen keeps one counter, the next page to show (screen +60):
- init 0x8260E868: counter 0; loaded 0x8260E888 and A run the advance 0x8260E950: below the page count it posts SetState(counter) and counts up, else it asks the "finished" popup (title / description / Yes / No);
- B (input 0x8260E890): below 2 the screen closes, else SetState(counter - 2) and counts down;
- popup callback 0x8260EA48: choice 0 closes, any other starts over at page 0.

Values are data (`CalibrationRules`), retail's the defaults, so a mod can add or drop pages.

## Code reference: offline_profile.rs

Detail moved out of the code comments of `crates/skate-core/src/offline_profile.rs`; each comment there links its section here.

### Skate profile screen

Free Play pause row Skate Profile (doc 31 "Skate Profile"): retail FE screen 61 (vtable 0x82305448, template 106 OfflineProfile, `profile/offline_skateProfile`). The career view in three tabs (progression, stats, personal bests); the movie reads its pages from the Progression and Stats host objects. The C++ side keeps one value, the tab index (screen +0x3C, research b308):
- init 0x8264C530: tab 0, then the refresh 0x8264C830 (tab -> the tab's SetState);
- input 0x8264C608: Back plays core_b_button and pops the screen; the tab keys play core_tab, step the tab (clamped: at either end it stays), then SetState(30) (the movie syncs its tab highlight) and the refresh, also at either end; Up / Down play profile_career_scroll and stay unconsumed, so the movie scrolls its list.

Values are data (`ProfileRules`), retail's the defaults, so a mod can add, drop or reorder tabs.

## Code reference: fe_popup.rs

Detail moved out of the code comments of `crates/skate-core/src/fe_popup.rs`; each comment there links its section here.

### FE popups

The front end's popups (doc 31 "Popups"): retail's popup list on the FE stack object (TU3, a local research note). A caller fills an sPopupInfo (defaults `sub_82D0B0B0`) and pushes it (`sub_82D0A8D0`): it gets the next id and joins a list sorted by priority, ascending; ties keep arrival order, so the newest of the highest priority is last, and the last one is shown (FE vt+24 `sub_825EC530`). The player's choice goes to the popup's callback as (id, choice, user) (host vt+64 `sub_825EF698`), then, with auto-remove on (the default), the popup leaves the list (`sub_82D0A628`) and the next last one shows. Clear-all (`sub_82D0A738`) calls back every popup not marked to survive it with choice -2 and drops it.

Callbacks are stable string ids (never code pointers), so requests and events are plain data a mod can send and receive, and a multiplayer build could serialise them.

## Code reference: fe_input.rs

Detail moved out of the code comments of `crates/skate-core/src/fe_input.rs`; each comment there links its section here.

### FE pad input

Front-end pad input (doc 31, "Pad input"): retail's `input.cfg` Apt actions (`AptUp = LStickU.repeat || DPadU.repeat;` ...) evaluated over the 24 native pad slots (`input::xbox::convert`), giving the key indices the FE poll (sub_825DFDC8) passes to the screen stack's input handler, in the order of the Apt key table (0x83027BF0, extracted at setup). A local research note.

The edges are retail's channel update sub_82966F30 (a local research note, pass 2; constants verified): held when the value is above 0.5; after each change the next 3 ticks keep it frozen and report no press or release; repeat fires with the press, 24 ticks later, then every 12. Suffixes (parser sub_829674F0): held, pressed, released, repeat. NOT RETAIL YET: the tick rate is not proven; the console's 30 Hz cadence is assumed (0.8 s to the first repeat, then every 0.4 s), normalised so any frame rate gives the same timing.

## Code reference: fe_language.rs

Detail moved out of the code comments of `crates/skate-game/src/fe_language.rs`; each comment there links its section here.

### FELanguage formatting

FELanguage formatting natives the menu movies call (doc 31, Milestone 6). Retail registrar 825AA5E8 (TU3); a local research note.

Placeholders in the language strings are `{N:type[.P]}` (N one digit, P one digit of precision, default 3); `{{` stays literal. Number separators are data (`NumberFormat`), retail's table 0x82FDE250 holds `. , 3`.

NOT RETAIL YET: APT values are int or float at runtime, ours are all f64; an integral number is treated as an int (grouped), anything else as a float. Per-language separators and the typed formats other than `d` / `f` (points, distance, time, ...) are not decoded; those placeholders take the argument text as is.

## Code reference: challenge_map_host.rs

Detail moved out of the code comments of `crates/skate-game/src/challenge_map_host.rs`; each comment there links its section here.

### Challenge Map host

The Challenge Map screen's natives (FE screen 17): FEChallengeMap (registrar sub_8264A8A8, 24 getters, research b255) and the movie's picker callbacks (Game.OnMenuHilight / OnMenuSelect, b256). The navigation is `skate_core::challenge_map`; this host answers the movie from it and queues the screen states the C++ posts (`SetState(n)`, applied by `retail_menu_stage` after the call returns) and the leaf a select picked (dispatched through the menu registry, whose handler decides: a teleport, a mod).

## Code reference: trick_guide.rs

Detail moved out of the code comments of `crates/skate-core/src/trick_guide.rs`; each comment there links its section here.

### Trick Guide screen

The Trick Guide screen (pause menu Learn > Trick Guide; FE screen 14, template 29 TrickTutorial, movie `screens/tricks/trickguide`; doc 31 "Trick Guide"). Port of retail's screen-14 object (TU3 vtable 0x823038B8): a three-level list (categories, subcategories, tricks) over the trick records, the picker callbacks and the FETrickTutorial natives the movie reads. Local research notes (re-read the handlers named below).

The data is a [`TrickGuide`]: setup's records by Attrib key (scoring_trickcategory / tricksubcategory / scoring_trick), the category order and the slot name tables. Every part is plain data, so a mod can add or change entries by key; the navigation reads nothing else.

## Code reference: fe_blur.rs

Detail moved out of the code comments of `crates/skate-core/src/fe_blur.rs`; each comment there links its section here.

### FE backdrop blur

Front-end backdrop blur and tint: the world behind a menu screen blurs and darkens while that screen is on top of the FE screen stack (doc 31, "Pause blur").

Retail (TU3 recomp addresses): every FE screen has a record in the screen stack's info table (0x83026088, 72 records of 24 bytes, byte +4 = blur). The stack applies the flag of the screen on top on every push / pop through vt+28 = sub_825EBED8, which stores it at FE manager +461 and, on an off -> on change only, restarts the ramp (tint 255, blur 0.0). An empty stack switches it off. The FE manager tick sub_825D8E68 then ramps the blur strength (+468 / +472) and the RGB tint (+476 / +480 / +484) one step per tick, up when on and back when off. The values here are the retail defaults; mods override them per screen (menu_backdrop).

## Code reference: crossbar.rs

Detail moved out of the code comments of `crates/skate-core/src/crossbar.rs`; each comment there links its section here.

### Crossbar navigation

Pause menu ("crossbar") navigation, ported from the retail C++ screen (doc 31, Milestone 6).

Retail keeps all navigation state in C++ (crossbar screen fields +368 tab, +372 row, +376 sub-option, +384 level); the core_menu movie only mirrors it through the CoreMenu getters and redraws when the C++ posts `SetState(n)`. The handler is sub_8261B560 (TU3): per input event it moves the indices, plays an fe sound and posts a state. This module is that handler over a `CrossbarView` (the current mode's tabs and rows, from the menu registry), so the rows, their enabled rules and the sounds / state numbers stay data that mods can change.

Engine-independent: no rendering, I/O or ECS.

## Code reference: credits.rs

Detail moved out of the code comments of `crates/skate-core/src/credits.rs`; each comment there links its section here.

### Credits screen

The credits (doc 31 "Credits"): retail FE screen 25 (vtable 0x82302C50, template 30 `extras/credits.swf`, research b489), opened from the Extras Credits row. Its init reads `data\credits\credits.txt` (miscload.big) through the cCreditsManager; the parser (0x82D07C40) turns each line `<font> <size> <spacing> "<text>"` into entries; the movie asks for them through the `FECredits` object (registrar 0x825AAE58: GetNumCredits, GetFontSize, GetLineSpacing, GetLineOffset, GetLine, ExitCredits), scrolls them up itself and calls ExitCredits after the last one; B pops the screen at any time. The screen posts SetState(1) (Rolling) once the file is parsed.

Moddable: the lines are data. Mods append their own sections (`#ID_` string ids or literal text, size, spacing) after the retail lines; the movie needs no change.

## Code reference: scene_clip.rs

Detail moved out of the code comments of `crates/skate-data/src/scene_clip.rs`; each comment there links its section here.

### Scene clips

Scene clips: one-clip banks from `scene.big` (`data/scene/trickguide/<name>1.abin` for the Trick Guide demo). Each file holds the full character hierarchy (131 bones: body, board, fingers, twist helpers, face), one RAW clip and one RAW pose. The clip channels are deltas on the file's pose (SK83_TPOSE): each frame's local is retail's AddSQT (`pose_add::add`, 0x828CCC08) of the stored delta (identity where nothing is stored) onto the pose. Evidence: limb channels sit near identity where the pose has large rotations (LEFTSHOULDER, LEFTUPLEG), and the HIPS channel is the inverse of the bind trajectory, so HIPS adds up to identity under the trajectory. NOT RETAIL YET: the add is inferred from the data; what selects it in the RAW job (field +62) is unread.

## Code reference: text_entry_ui.rs

Detail moved out of the code comments of `crates/skate-game/src/text_entry_ui.rs`; each comment there links its section here.

### On-screen keyboard look

The on-screen keyboard (ours; doc 31 "Extras and cheat codes"): drawn while retail FE 46 (text entry) is on top, where retail showed the Xbox system keyboard. The state is `extras_host::TextEntryView` (the stage owns the input); this only builds its draws for the menu stage's layer, in the retail menus' own popup style: the frame of the Extras popup rebuilt at the keyboard's size from the same `controls/panel` pieces (corners, edges, fills, paper grain, with their UV turns and colours as the Extras movie draws them), the title in Futura Overprint, the selected-row blue (0x37A5FF) for the highlighted key and the connected pad's button prompts (`prompt/<button>`, resolved like the movies' CustomButtons) on the paper footer.

## Code reference: settings_host.rs

Detail moved out of the code comments of `crates/skate-game/src/settings_host.rs`; each comment there links its section here.

### Game Settings natives

The Game Settings screen's natives (options.apt, template 90, FE screen 45; doc 31 "Game Settings screen"): `Game.GameSettings_*` and the `Game.OnMenu*` calls, answered from the menu registry like CoreMenu, so mods' rows, labels, rules and values show up. A local research note.

The movie only knows row indices; retail indexes its raw table by the screen index (+68) and row (+72). Ours indexes a snapshot of the visible row ids taken at GetNumOptions (the start of CreateOptions) and kept until the next rebuild, so SetState(50) refreshes the same rows.

## Code reference: graphics_menu/view.rs

Detail moved out of the code comments of `crates/skate-game/src/graphics_menu/view.rs`; each comment there links its section here.

### PC overlay look

The PC overlay's look and motion, ported from the recomp's settings overlay (skate3recomp's rexglue-sdk, `src/ui/overlay/simple_settings_overlay.cpp`, `SimpleSettingsDialog::OnDraw` and `GatherInput`): a frosted teal category rail with a pinned Close Game, white setting rows under a sliding black focus fill with a lime ring, magenta steppers and sliders, lime section bars, a description panel and a pad / keyboard legend, over the blurred and tinted world. Metrics, colours, easing rates and repeat timing are the recomp's (authored for 1080p, scaled by the window height); the scene blur is `fe_backdrop`'s pass at the recomp's strength. The rows are ours (`Menu::page`, `Menu::describe`).

### Challenge Map teleports

Challenge Map teleports (the `teleport:<fe_locations key>` engine handler, retail_menus.rs): the request event, its planning (same map, another map, refused) and the system that applies it once the right map is loaded.

Retail (b244d): an fe_locations select posts a Flow::Teleport request; its event carries Transform (the location's locator matrix, unchanged), IsOnBoard = 0 and IsSessionMarkerTeleport = 0. A target on another map loads that map first. Ours: the locator matrices come from setup's `private/teleports.json`; another map goes through the same map transition as the PC overlay's MAPS teleports; multiplayer refuses a map switch (as the overlay does).

## Code reference: text_viewer.rs

Detail moved out of the code comments of `crates/skate-core/src/text_viewer.rs`; each comment there links its section here.

### Text viewer

The text viewer (doc 31 "Extras: Licenses"): retail FE screen 3, ToSScreen (vtable 0x82304F10, template 28 `nucleus/nucleus_tos.swf`, research b322), the Nucleus terms-of-service screen the Extras Licenses row reuses in its view-only state. The C++ splits the body into words at its delimiters (0x8263AD78: space, LF, CR, TAB; word i runs from boundary i to boundary i + 1, so every word after the first keeps its leading delimiter and the movie's layout sees the line breaks) up to a character cap (setup flag +0x31: 81626); the movie asks for them through the `tos` object (GetHeaderText, GetTOSstate, GetNumWords, GetWordRange) and lays them out itself, 20 lines, scrolled by the left stick. Back plays core_b_button and closes the screen in view mode.

## Code reference: offline_profile_host.rs

Detail moved out of the code comments of `crates/skate-game/src/offline_profile_host.rs`; each comment there links its section here.

### Skate Profile natives

The Skate Profile screen's natives (doc 31 "Skate Profile", research b308): the Progression object (registrar 0x825AFBA8) and the Stats object (registrar 0x825AB630) the movie `profile/offline_skateProfile` reads its pages from. The tabs are `skate_core::offline_profile`; this host answers the movie's calls from the page data (empty until a career system or a mod fills it, `sdk.menus.profile`) and queues the screen states the C++ posts back (applied by `retail_menu_stage` after the call returns).

## Code reference: fe_backdrop.rs

Detail moved out of the code comments of `crates/skate-game/src/fe_backdrop.rs`; each comment there links its section here.

### FE backdrop pass

FE backdrop: the world behind a front-end screen blurs and darkens while that screen is on top of the FE screen stack (doc 31, "Pause blur"). The ramp is `skate_core::fe_blur` (retail FE manager); which screens blur comes from the menu tables (retail screen stack info table, extracted at setup); mods override it per screen with `sdk.menus.backdrop`. The pass runs on the gameplay camera after the retail tone pass and before the UI, so the menus themselves stay sharp.

### HDR and LDR views (2026-10-26, research b621)

Problem: menus over the test world (and over any map without retail render data, e.g. a mod map) panicked in wgpu:
"RenderPass uses [Rgba8UnormSrgb] but the RenderPipeline with 'fe backdrop' label uses [Rgba16Float]". Cause [code]:
the pass queued one pipeline with the fixed HDR target format (`fe_backdrop.rs` `initialize`), while the gameplay
camera has `Hdr` only when `RetailScene` holds (`camera.rs` spawn and `set_world_environment`), so those views are
LDR. A player could reach it by picking "Test world" in the overlay map list and opening a menu.
Change: `initialize` queues the same pipeline for both formats (HDR and `bevy_default()`); the node picks by
`ViewTarget::is_hdr()`, as Bevy's own blit pipeline keys on the target format. Mod backdrops (`sdk.menus.backdrop`)
take the same path.
Verification: `--test-world --verify` with `SKATE_VERIFY_MENU=optin` panicked before and now saves a blurred capture;
the same capture on DownTown (HDR) is unchanged.

## Code reference: extras_host.rs

Detail moved out of the code comments of `crates/skate-game/src/extras_host.rs`; each comment there links its section here.

### Extras natives

The Extras screen's natives and the cheat code flow (doc 31 "Extras and cheat codes", research b320 / b304 / b305): Game.Extras_GetNumOptions / _GetOptionTitle / _MenuTitle and the menu picker's Game.OnMenuGetItemEnabled / OnMenuHilight / OnMenuSelect while FE 18 is on top. Rows and states are `skate_core::extras`; the cheat matcher `skate_core::cheats`; the text entry and its on-screen keyboard `skate_core::text_entry`. The stage applies what this host queues (screen states, row actions) after the movie call returns.

## Code reference: crossbar_host.rs

Detail moved out of the code comments of `crates/skate-game/src/crossbar_host.rs`; each comment there links its section here.

### CoreMenu host

The CoreMenu script object the pause menu movie (core_menu.apt) reads (doc 31, Milestone 6).

Retail registers 21 native members on CoreMenu (registrar sub_825B68E0, TU3); all are read-only views over the crossbar screen (navigation state, skate_core::crossbar) and the current mode's menu (our menu registry), except SetOnlineTeamSubOptionDone. Research: a local note.

## Code reference: fe_fade.rs

Detail moved out of the code comments of `crates/skate-core/src/fe_fade.rs`; each comment there links its section here.

### FE fade

The front end's fade to black (doc 31 "Fade to black"): retail's FE fader object (`[[0x830CFE14]+304]`, TU3). A screen change holds a bit: the alpha rises 0.04 per FE tick, and when the screen is black the waiting actions run (hold sub_825E6B30, tick sub_825E69A0). Releasing the last bit fades back in at 0.04 per tick (release sub_825E6BF8; with its instant flag in one tick). Alpha 1 = black. Values are data (`FeFadeParams`), retail's the defaults.

## Code reference: trick_guide_host.rs

Detail moved out of the code comments of `crates/skate-game/src/trick_guide_host.rs`; each comment there links its section here.

### Trick Guide natives

The Trick Guide screen's natives (doc 31 "Trick Guide"): FETrickTutorial (registrar sub_825AE7E0), Game.GetMenuItems over the screen's list and the picker callbacks, HUDComponents.PlayerStance_IsRegular. The navigation is `skate_core::trick_guide`; this host turns the movie's calls into it and queues the screen states the C++ posts back (the facade's SetScreenState, applied by `retail_menu_stage` after the call returns).

## Code reference: popup_host.rs

Detail moved out of the code comments of `crates/skate-game/src/popup_host.rs`; each comment there links its section here.

### Popup host

The popup movies' C++ side (doc 31 "Popups"; a local research note): the AS `Popup` natives (registrar `sub_825A9F40`) answer from the shown request, and while a popup is up it is the focus screen for `Game.OnMenuSelect` / `OnMenuGetItemEnabled` / `OnMenuHilight` (`sub_82D09F28`), so a choice reaches the popup list and not the screen below (the list itself is `skate_core::fe_popup`, run by retail_menu_stage).

## Code reference: image_renderer_host.rs

Detail moved out of the code comments of `crates/skate-game/src/image_renderer_host.rs`; each comment there links its section here.

### Image slots

The front end's image slot manager (`*(FUN_824AD2F0() + 0x120)`, research b258): 40 global slots the movies reserve, load a named runtime image into and draw through an `ImageRenderComponent` clip (the Challenge Map pictures). The `Game.ImageRenderer_*` natives live here; the draw resolves `slot/<n>` texture keys through [`ImageSlots::texture`]. Mods reach the pictures through the image names (`RetailMenuMovies::set_named_image`).

### Challenge Map navigation

The retail Challenge Map screen's navigation (FE screen 17, research b255 / b256), over the menu registry's map nodes: the tab (`mgr+0x198`, clamped, no wrap), the folder stack (`mgr+0xE5B4`), the highlighted row (`+0xD8`) and the selected one (`mgr+0xE5E8`). Engine-independent: the host (skate-game `challenge_map_host`) turns the movie's natives and picker callbacks into these calls and posts the returned screen states (`SetState(n)`).

## Code reference: apt_movie.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/apt_movie.rs`; each comment there links its section here.

### registerClass order

registerClass: the clip takes the class prototype now. Retail sub_82E5A9E0 (after sub_82E7AF18, from both the timeline place 82E7B090 and attachMovie 82E565A0; research b452) sets the prototype, runs the first tick 82E5B640 (which places the children, so their construct handlers run first), then the constructor (82E6A208). So the constructor is queued after the children are placed; it still runs before the clip's own frame actions, which wait on `pending`. A child's construct value (button_item2's mButtonRender `_test = "Unknown"`) no longer overwrites what the parent's constructor set (`SetButtonType(mButtonImage)`, the teleport prompt's DPad_Up).

### createTextField

MovieClip.createTextField(name, depth, x, y, width, height): a new, empty text field at a script depth (sk8popup's body). Standard AS2 semantics, NOT RETAIL YET (the retail handler is not decoded): its own text descriptor (font from setTextFormat / setNewTextFormat, else the FontManager fallback at 12 units, left aligned, white), placed at (x, y), width x height bounds.

## Code reference: retail_menu_movies.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/retail_menu_movies.rs`; each comment there links its section here.

### Menu manifest versions

Manifest version setup writes (3: textured units carry their bitmap id, Milestone 4; 4: screens/main; 5: main.apt's layer movies; 6: the challenge map; 7: named runtime images, the challenge map pictures; 8: the Skate Profile, FE 61; 9: Extras, FE 18; 10: the text viewer, FE 3; 11: the replay editor and help, FE 12 / 13; 12: every DLC pack's text tables, so the Challenge Map names the DLC locations; 13: the credits, FE 25, and their lines file; 14: SpawnSkater, FE 62).

### Oldest menu manifest

The oldest version this engine still reads: 6, 8, 9, 10, 11, 13 and 14 only add a movie (a 12 set keeps Credits greyed), 7 an optional image table and 12 DLC text, so a 5 set works without the challenge map (its pause item keeps the PC overlay's MAPS until Refresh Game Files), a 6 set draws no map pictures and a 7 / 8 set keeps the SKATER / EXTRAS overlays for Skate Profile / Extras.

## Code reference: menu_values.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-core/src/menu_values.rs`; each comment there links its section here.

### Settings defaults

Retail's value of a settings row before any save changes it: the settings object (`[[0x83067060] + 28]`) read in the recomp after a boot without a signed-in profile [trace, local recomp run]: volumes +128 / +132 / +136 = 1.0, Subtitles +157 = 0, Minimap +158 = 1, Vibration +163 = 1, HUD +164 = 1, Camera manual +165 = 0, Units +168 = 0, Play Mode +180 = 1, HOM mode +184 = 0, Camera Angle +188 = 0, axes +192 / +193 = 0, Transparency +195 = 0, Skate Feed +196..+200 = 1. Rows show it until an engine setting or a mod binds a value (the value hooks).

## Code reference: hud_render.wgsl (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/hud_render.wgsl`; each comment there links its section here.

### HUD blend space

The layer target holds display (gamma) values and blends them there, like the console's front end: retail's APT draw state is straight SRC_ALPHA / INV_SRC_ALPHA (`sub_827E4AF0`, bound by `sub_825D4490`) on a display-value image (research menus-m6 "Retail blend state", medium-high confidence: the 2D pass's format is inferred). Blending in linear made the soft low-alpha glow layers of a selected menu row far too bright. The composite (`hud_composite.wgsl`) converts the layer to linear once. mode.x = 1 (the retail menus and the gameplay HUD, one APT player in retail): the colour transform applies to the display value of the texel. mode.x = 0 keeps the linear transform for any other user, stored as display values.
