---
name: retail-menus
description: Work on the retail front-end port that runs Skate 3's own APT (Flash AS2) menu movies on the engine's front-end stage - the pause menu, Game Settings, Video Calibration, Trick Guide, Challenge Map, Extras, Skate Profile, Free Play (Spawn Skater, options), popups, the HUD minimap and the loading screen. Use when working on the APT stage / VM, a screen's host objects (CoreMenu, Game, FELanguage and the per-screen natives), the menu movie export, or automated menu screenshots.
---

# Retail menus (APT front-end stage)

Doc: `docs/hails-additions/31-retail-menus.md` (one section per screen and change, with the retail addresses it ports).
The foundation went upstream as #59; the rest is part of #52.

## How it works (a port of retail, not an own design)
- Retail runs its menus as APT (AS2) movies; the C++ side only calls them by path and answers native objects. The
  engine does the same: `apt_stage.rs` is one script VM with several movies (levels), MovieClipLoader / loadMovie /
  getURL, registerClass, per-movie frame rate, `call_path("_global.ScreenManager.OpenScreen", ...)` for the C++ calls,
  and `Stage::draw` for drawing on the HUD layer (`scoring_hud::render_draws`).
- Boot like retail: `screens/main.apt` as `_level0` (it loads the screen manager), the crossbar enums registered
  through `_level0._root.RegisterChallengeEnum(name, "%d")`, RegisterTemplate for every template in the menu tables,
  OpenScreen with the template's retail arguments, SetState(InitialiseMenu) on IntroComplete.
- C++ side: `crossbar_host.rs` (`FrontEnd`: CoreMenu + FELanguage; Game / Audio calls recorded) dispatches to one host
  per screen (`spawn_skater_host.rs`, `tips_loader_host.rs`, `trick_guide_host.rs`, `popup_host.rs`, ...);
  `skate-core/src/crossbar.rs` (navigation port, data-driven `CrossbarRules`); the menu registry (`skate_core::menus`)
  holds rows and enabled rules and is moddable (`sdk.menus`).
- The FE screen stack (`retail_menu_stage.rs`): screens are keyed by their record in the menu tables (the key is
  `hash64(<template name in lower case>)`, e.g. `tiploader` -> `e1fa207da2bdf61a`); the fader (`skate_core::fe_fade`,
  0.04 per FE update) holds black around screen changes with retail's holder bits.
- Popups: `skate_core::fe_popup` (list), `popup_host.rs` (natives, focus); popup movies open on the "popup" layer at
  boot; mods: `sdk.menus.popup`.
- Screens ported: pause menu (crossbar), Game Settings, Video Calibration (FE 51), Trick Guide (FE 14) with its 3D demo,
  Challenge Map (FE 17), Extras (FE 18) with the cheat entry keyboard (FE 46, engine-drawn), Skate Profile (FE 61), Free
  Play: Spawn Skater / Call Skater (FE 62) and the options (FE 34), the replay screens (FE 12 / 13), the loading screen
  (FE 52, `screens/main/tiploader`). The HUD minimap (`hud2/hudminimap`) is not a front-end screen: it runs on its own
  VM in the scoring HUD (`hud_minimap.rs`).
- On by default; `SKATE3_RETAIL_MENUS=0` turns the retail menus off; Start / F8 open the pause menu. Installs whose menu
  tables lack the FE screen records fall back to the PC overlay.

## Run and test
- Export a dev set: `py -3.13 tools/prepare_menu_movies.py --game <extracted disc> --output <dir> --collections
  <install>/assets/private/stock/skater-collections.json`; setup's `hud` group does the same for an install.
  `SKATE3_MENU_MOVIES=<dir>/runtime` points the game and the data tests at the dev export (useful before a refresh).
- Tests: `cargo test -p skate-game --locked -- apt_ menu hud core_menu fe_language` (data tests skip when the installed
  set is an older version); `cargo test -p skate-core crossbar`. `SKATE3_DUMP_DRAWS=1` on
  `retail_screen_manager_opens_a_screen` prints the level tree and every draw (bounds, colour, texture).
- Automated screenshots: `SKATE3_RETAIL_MENUS_OPEN=1` opens the pause menu at once; `SKATE3_RETAIL_MENUS_KEYS` injects
  key indices one per frame (`_` = a frame without a key, `_xN` = N frames): 2 A, 3 B, 5 X, 7 RT, 10 LT, 12 Up, 13 Down,
  14 Left, 15 Right. Example, the Challenge Map's Locations list: `SKATE3_RETAIL_MENUS_OPEN=1
  SKATE3_RETAIL_MENUS_KEYS="2,_x150" SKATE_VERIFY_AT=16 skate3rust --assets <install>/assets --map <map> --verify out.png`.
  Free Play from the pause menu: `_x60,13,_x30,2`; Call Skater: `_x60,13,_x30,13,_x30,2`; Extras:
  `_x60,15,_x30,15,_x30,15,_x60,13,_x30,2`. `--verify` gives up after about 45 s, so keep `SKATE_VERIFY_AT` well below.
- Loading screen: `SKATE_MAP_SWITCH=8:<map> SKATE_VERIFY_AT=11.5 skate3rust --map <small map> --mute --verify out.png`
  captures it mid-load; data test `apt_stage::tests::retail_loading_screen_shows_a_tip`.
- Trick Guide clips: `tools/check_trick_clips.sh [clip dir]` (every collections name has a clip, all decode, posture at
  both ends).
- Reading a movie's code: `py -3.13 tools/apt_dump.py <movies/.../name.json> > out.txt` (pool names resolved; a reading
  aid, never commit its output: it is game content).

## Rules learned
- Every gap gets a retail check first (handler address: the APT opcode table, natives through their registrars), then
  a port; anything taken from Flash semantics instead is marked "NOT RETAIL YET" in code and doc.
- When the picture is wrong, dump the tree and the draws before guessing: alpha 0 from an unplayed timeline, a missing
  C++ SetState, a lower-case native name and a script-moved clip reset by its timeline were all found that way.
- A disassembler can drop a branch: when a movie seems to do nothing in a state, run it in the VM test harness (log the
  host calls and the movie's own `trace` output) before concluding.
- Screens are moddable from the start: rows, rules and timings as data with retail defaults, `sdk.menus` hooks, movie
  overrides through the content layer, cleanup on mod disable.

## Open
The "NOT RETAIL YET" lines of doc 31's sections; inline button tags in menu text (`<B:B_hud> Back`) are not laid out
yet; the loading screen's "Loading..." position, spinner and text tint; the FE 62 preview (MarqueeViewer); the
minimap's challenge icon groups (they need a challenge system).

## Popup kinds (2026-10-25, doc 31 "Popup kinds")
A popup with an OK / Continue option is `Standard` (Sk8Popup, the default). `Small` (small_popup) shows a title and
text only and closes by B when cancelable or by code; retail never pushes a small popup with options. The host show
`0x825EF440` maps kind 2 / 3 to the small movie, 1 to the autosave one, anything else to Sk8Popup.
