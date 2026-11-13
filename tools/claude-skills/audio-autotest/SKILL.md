---
name: audio-autotest
description: Run the automated in-game check of the audio modding features (dev mod mods/audio-content-test in autotest mode) muted in the background, or audible while a person listens, and turn their listening verdict into PR evidence. Use after changes to audio modding (upstream PRs #36, #43), before marking such PRs ready, or when someone is ready to listen.
---

# Audio modding autotest

Needs the dev mod `mods/audio-content-test` (in upstream main since PR #32) with its `autotest` setting, a built
`bin\skate3rust.exe`, and converted assets (skill `asset-setup`) including the DownTown map.

## Pieces
- Runner: `tools/audio_mod_autotest.ps1` next to this file. Its header comment is the full spec.
  `-Pass on|off|both` (on = dev mod with autotest, off = mod disabled, asserts retail), `-Audible`, `-Worktree`
  (default: the checkout the script sits in), `-StepSeconds/-GapSeconds`, `-WaitMinutes`, `-Map` (DownTown).
- The mod's autotest code: `mods/audio-content-test` (setting `autotest`), logs `AUTOTEST <check> ok|fail
  <details>`. Mod ON on DownTown: 16 steps, about 35 checks; the runner answers its hot-swap requests (edits /
  reverts the copy's `audio.json`). Mod OFF asserts retail (map audio `["retail"]`, the retail crossfade, the
  install's project count, no mod lines).
- Keep a plain-language list of what each step should sound like (for the listener) beside the mod, in sync with it.
- Output: `.local/autotest/runs/<stamp>-<pass>/` (`stderr.txt` = game log, `results.txt`). Exit 0 ok, 1 failed check,
  2 not launched (busy), 3 setup error.

## Muted run (any time no game runs)
`powershell -File .claude\skills\audio-autotest\tools\audio_mod_autotest.ps1 -Worktree <checkout> -Pass both`.
Minimised window, `--mute`, 5 s steps. It waits while another skate3 / skate3rust process runs or
`.local\GAME_LOCK.txt` exists, and writes and removes only its own lock. One game instance at a time.

## Audible run (a person listens)
1. Only when the listener says they are ready, and check they can see and hear the game.
2. `-Audible`: 15 s steps, 3 s quiet gap, compact top-left HUD (step, name, seconds left, what to listen for); the red
   "MUTED TEST" label shows only on muted runs. Point the listener to the step list.
3. Write the listener's verdict down word for word; in PRs, quote it exactly rather than summarising.

## Design rules learned the hard way
- Every step needs its own clearly different sound; one shared beep is too confusing.
- HUD rows must fit on one line each; top left, small.
- Explain what a step should look like, including camera moves. The camera is parked at the Baby_Cry_1 emitter for
  steps 1-3 only and must come back on timeout and at stop (a parked camera kept the emitter playing after a run).
- Steps long enough to listen; silent read-back checks short.
- Known flaky checks: the synthetic ollie checks (`F11_landing_seen`, `D5_pop_seen`, `tags_after_resubscribe`)
  sometimes fail because the ollie ends in a grind; 3 s steps are too short for them.
- Test content can create artefacts: six identical cars beat against each other and sounded like a lawn mower.
  Separate same-model emitters (rings, detune) before blaming the engine, and never claim retail parity from a
  guess.
