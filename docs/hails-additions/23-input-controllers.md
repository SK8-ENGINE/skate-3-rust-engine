# 23: Input: controller identity and gameplay action IDs (addition to PR #24)

Branch: `feature/input-controllers`, one commit on top of `input/sdl3-gamepad-backend` (upstream PR #24, the SDL3
gamepad backend). It extends #24 instead of opening another input PR. Todos: `controller-type-detection`,
`document-action-ids`.

## Problem

1. **Controller identity was lost.** SDL reports each pad's type (Xbox 360 / One, PlayStation 3/4/5, Switch Pro,
   Joy-Con, standard), its USB vendor/product, its paddles and the driver that owns it, but #24 only wrote that into
   one log line. `DevicePacket.subtype` is a constant for SDL pads and `ControllerInput` kept only `Ready`, so no
   engine system, mod or menu could tell an Elite Series 2 from a DualSense. The XInput fallback
   (`SKATE3_INPUT=xinput`, or SDL failing to start) reported nothing at all.
2. **Action IDs 64–81 were undocumented.** `sdk.input.override_action(id, value)` takes IDs 64–81, but nothing said
   which action each one is, so a mod could not drive the skater (found while scripting a footstep trace).

## What retail does

- Retail Skate 3 (Xbox 360) only ever sees Xbox 360 pads through XInput. It reads the capability subtype once per
  slot (`8296D480`: byte 13 set only for subtype 7, the alternate guitar) and converts the pad state in `8296D5F8`.
  Identity therefore changes **nothing** in gameplay: this work is metadata for troubleshooting, mods and future
  presentation.
- The gameplay actions are the eighteen `GP_*` expressions of the shipped `data/config/input.cfg` ("XBOX control
  scheme 1"), registered at cInputMap indices 64..81 by `82697740` in file order (`GP_LStickX` … `GP_BFace`). The
  setup already validates the installed `input.cfg` against these expressions (`skate-data` `input_config.rs`).
- Their meaning comes from the retail input listener (`ActionGraphInputListener::Fill`, `825999F0`) that turns the
  Raw/Derived controller words (`82598FE0` / `825992D8`) into intents: e.g. B → `Brake` / `Dismount`
  (`8259A9E4..AA54`), RB → `GrabWorld` (`8259A554` / `8259AF60`), Y → `ToggleOffBoardState` (`8259A604..A6C0`). These
  functions are already ported in `skate-core/src/input/*_intentions.rs`; this change reads them, it does not change
  them.
- Button-prompt glyphs: the engine loads only the retail trick-display HUD movie, which has no button prompts, and no
  other converted asset holds prompt art. So there is nothing to switch; `ControllerKind.prompt_style` /
  `face_labels` is the hook for a future prompt UI and for mods' own UI.

## Change

### Controller identity
- `input/controller_kind.rs` (new): `ControllerKind` = family (`xbox360`, `xbox_one`, `xbox_elite`,
  `xinput_gamepad`, `playstation3/4/5`, `switch_pro`, `joycon_left/right/pair`, `standard`, the XInput non-gamepad
  subtypes, `unknown`), name, vendor/product, backend (`sdl` / `xinput`), driver (SDL device path, e.g. `XInput#0`),
  XInput subtype + wireless flag, paddles reported vs. paddles the model has, touchpad, misc button, prompt style.
  - SDL: from `SDL_GamepadType`, vendor/product, `has_button(paddles / Misc1)`, touchpads, path; the model table only
    refines (Xbox One → Elite) or names a pad SDL calls standard/unknown.
  - XInput fallback (the user's decision: it must report identity too): `XInputGetCapabilities` subtype/flags, plus
    vendor/product from `XInputGetCapabilitiesEx` (xinput1_4 ordinal 108, undocumented, the call SDL makes) when it
    exists; else the coarse kind from the subtype. The subtype `xbox::convert` uses still comes from the documented
    call, unchanged.
  - Model table: public USB IDs for Xbox 360/One/Series/Elite, DualShock 3/4, DualSense (Edge), Switch Pro, Joy-Con.
    **Data-driven:** `settings/controller.json` `"models": [{"vendor": "2dc8", "product": "3106", "name": "...",
    "family": "xbox_one", "paddles": 2}]` entries are looked up first (add or rename pads without a rebuild).
- `platform.rs`: `DevicePacket.kind`; SDL slots publish their kind with each snapshot; the XInput capability cache
  (1 s refresh) now caches subtype + kind together.
- `controllers.rs`: `ControllerInput.kinds[slot]`, `kind(slot)`, `active_slot()` for engine systems; cleared with the
  slot on disconnect.
- Log: `Controller N: identified as <summary>` when a slot's identity changes (the existing SDL connect line stays).
- Esc menu: EXTRAS → a read-only "Controller" row listing every connected slot's summary, e.g.
  `0: Xbox One Elite 2 Controller [Xbox Elite] 045e:0b22, SDL via XInput#0, paddles 0 of 4`: the "paddles 0 of 4" is
  exactly the troubleshooting case in gotchas (SDL's XInput driver hides the Elite paddles).
- Mods (read-only): `sdk.input.controller(slot)` (default: the slot gameplay reads), `sdk.input.controllers()`,
  `sdk.engine.read('input').controllers`, snapshot field `controllers`; capability `controllers = 1`. JSON names are
  stable snake_case. No write path: identity is a fact about hardware; a mod that wants other prompts uses
  `face_labels` / its own UI.
- Nintendo layout stays positional (South = the A action), as decided 2026-09-30; only the printed labels differ.

### Action IDs
- `skate-core` `input::gameplay_map::ACTIONS`: one table (ID, retail name, expression, mod key, range). The setup's
  `input.cfg` check now reads it instead of its own copy (same 18 entries, moved).
- `sdk.input.action_ids` (`a = 80`, `left_stick_x = 64`, …; capability `action_ids = 1`); `override_action` and
  `action` accept the keys as well as the numbers.
- `sdk/GENERAL_API.md` "Gameplay action IDs": ID → key → retail action → expression → Xbox 360 binding → the
  listener intents it reaches; "Controller identity" section. `sdk/skate.lua`: per-ID annotation on
  `override_action`, `ControllerKind` class, `controllers` snapshot field.

## Files

- `crates/skate-core/src/input/gameplay_map.rs`, `gameplay_map_tests.rs` (new)
- `crates/skate-data/src/input_config.rs`, `tests/input_config.rs`
- `crates/skate-game/src/input.rs`, `input/controller_kind.rs` (new), `input/platform.rs`, `input/controllers.rs`,
  `input/tests/controller_kind.rs` (new), `input/tests/action_docs.rs` (new), `input/tests/controllers.rs`
- `crates/skate-game/src/graphics_menu.rs`, `crates/skate-game/src/modding/mod.rs`
- `crates/skate-mods/src/api.lua`, `vm.rs`, `tests/general_api.rs`
- `sdk/GENERAL_API.md`, `sdk/skate.lua`

## Verification

- Gameplay byte-identical: `controllers::tests::controller_identity_is_metadata_and_never_changes_gameplay_actions`
  feeds the same pad state with and without a kind through the production collect/publish path and compares every
  mapped action and the published tick input bit for bit. The subtype passed to `xbox::convert` is unchanged for both
  backends (SDL constant 1; XInput from `XInputGetCapabilities`).
- Kind mapping: `controller_kind::tests`: every `SDL_GamepadType`, every XInput subtype, the VID/PID table, user
  model overrides, Elite refinement, the XInput+Ex path, stable JSON names, settings id parsing.
- Action meanings: `skate-core` `gameplay_map::tests` drive each ID exactly as `override_actions` does
  (`values[id-64] = value`) through the retail Derived controller update and all listener producers (riding, trick,
  off-board, gestures, manual, grind, wipeout) and assert the intents in the table, including the combinations (bail
  = both stick clicks + both triggers full; LB over RB; bumpers block D-pad gestures; B blocks the off-board sprint).
- Docs: `input::action_docs` tests check the GENERAL_API.md table rows and both Lua `action_ids` tables against
  `ACTIONS`.
- Lua surface: `skate-mods` `general_api_tests::input_reads_controller_identity_and_accepts_action_keys`.
- XInput Ex: `platform::windows::capabilities_ex_resolves_and_agrees_with_documented_call` (ordinal 108 resolves;
  same connected slots, device type and subtype as `XInputGetCapabilities`). The ignored
  `xinput_identity_of_connected_pad` prints each slot's XInput identity; on 2026-10-04 no pad was connected, so the
  live vendor/product read is not yet confirmed.
- Suites (2026-10-04, `--locked --no-fail-fast`): skate-game 326 passed + the 2 known failures
  (`pipelines_accept_valid_group_outputs_when_fingerprint_changes`, `sky_shader_validates`); skate-core lib 624 + the 2
  known (`predictive_contacts_and_retention_match_full_scan_for_every_primitive`,
  `a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`), integration tests all pass;
  skate-mods 62 + the known skyline failure; skate-data all pass. No game launched.

## Open questions

- In-game check of the Esc menu row and the log line with the user's Elite 2 (needs a play session; no game was
  launched for this change).
- Should a mod be able to add model entries (like `settings/controller.json` `models`), e.g. through `mod.json`? Not
  done: identity is hardware fact, the settings file already covers unknown pads.
- `XInputGetCapabilitiesEx` is undocumented. It is optional (falls back to the subtype), but it was only exercised by
  the ignored hardware test on one machine.
- Keep `xbox::convert`'s subtype-7 (alternate guitar) path meaningful for SDL, or drop it? Unchanged here.
- The PS3 disc's front end has PlayStation prompt art; if the engine ever plays retail prompt movies, `prompt_style`
  is where to pick the set.

## Code reference: rumble.rs

Detail moved out of the code comments of `crates/skate-core/src/rumble.rs`; each comment there links its section here.

### Rumble

Controller rumble, ported from the TU3 rumble controller (a local research note, constants verified in the TU3 image): each pad keeps channels of {low, high, decay}; a start overwrites its channel (`sub_82694D38`); every tick (`sub_82695010`) sums the channels per motor, caps the sum at `max + (1 - max) * headroom`, then decays each channel and drops it below the threshold; the device curve (`sub_8296D480`) turns the level into a motor speed. Retail's values are [`RumbleTuning::RETAIL`]; a mod can replace them. Engine-independent and deterministic (plain f32, retail's order of operations).

### Rumble device curve

The device curve: a motor level (0..1) as a 16-bit motor speed. `wireless` = retail's test in `sub_8296D480`: capability type 1 and flags & 0x0002 (X_INPUT_CAPS_WIRELESS); every other pad takes the wired curve. Retail's order: curve, clamp to 0..1, exactly 0 stays 0, else trunc(v * 65535). So a wireless pad's motor 1 at level 0 is 0.05 * 65535 = 3276, as in retail.

### Rumble on pads

Controller rumble on the pads: the TU3 rumble controller (`skate_core::rumble`) ticked at retail's 60 Hz whatever the frame rate, gated by Game Settings > Control > Vibration (settings +163), sent through the pad backend (`input::platform::set_rumble`). Who starts a rumble: switching Vibration on plays retail's test rumble (0.5 / 0.5, rate 5.0, `sub_8260DF40`) on the active pad; mods start and stop channels (`sdk.input.rumble` / `sdk.input.stop_rumble`, capability `input_rumble`). NOT PORTED YET: retail's gameplay rumble (the posters `sub_82BB8CF0` 0.5 / 0.5 / 1.2 and the per-item channels of `sub_8259E3D0`, events not identified; a local research note).
