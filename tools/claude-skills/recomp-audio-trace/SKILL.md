---
name: recomp-audio-trace
description: Build and run a local skate3recomp (static recompilation of Skate 3) with an audio trace to measure which sound banks/samples retail plays for gameplay events. Use when continuing retail-sound measurement work or rebuilding that local research build.
---

# Local skate3recomp audio trace (research only)

Credits: skate3recomp (https://github.com/mchughalex/skate3recomp, the static recompilation), its rexglue SDK, and
Xenia (from which rexglue's runtime pieces derive). Credit them in every doc and PR that uses measurements from them.

## Rules: read first
- **You need your own legally owned copy of Skate 3** and must build the recomp yourself. Nothing from the game
  (code, data, disassembly, decoded audio) is ever shared or committed.
- skate3recomp has **no licence**: local research only. Never copy its code (or its Xenia-derived rexglue SDK) into
  skate-3-rust-engine. Only measurements (bank names, sample indices, timings, addresses) come back, cited as
  "measured from a local recompiled build".
- Codegen output (`<recomp checkout>/generated`) is local.
- Run only one game instance at a time (the recomp is GPU/CPU heavy; don't run it beside a skate3rust session).

## Layout
- Clone the recomp to `<recomp checkout>` with submodules. `third_party/rexglue-sdk` is a submodule; if a nested
  module (for example `thirdparty/imgui` or `thirdparty/o1heap`) pins a commit that is not on GitHub, check out the
  SDK's nearest earlier public pin, and if a nested module is empty after a failed recursive update, check it out
  again individually. A debug-UI-only compile problem (for example gamma assignments in the imgui drawer) can be
  commented out locally.

## Title Update (required)
The recomp's own sources call recompiled functions by their **TU 3.0.3.0** addresses, so it does not build from the
base disc alone (codegen works; the `skate3` target then fails with `use of undeclared identifier 'sub_...'`). Put
the Title Update package from your own copy (title id 454108E6) in the recomp root, where CMake finds it
automatically ("title update codegen enabled"). It extracts `default.xexp` (and `data/webkit/EAWebkit.xexp`); the
expected hashes are in the recomp's title-update installer source. Exclude it from git locally
(`.git/info/exclude`: `TU_*`). If a TU-specific codegen patch anchor fails (for example the demo-path intro-movie
patch), make it non-fatal locally.

## Toolchain helpers (keep them in a gitignored folder of your checkout, for example `.local/recomp/tools/`)
`recomp_env.bat`:
```bat
@echo off
rem Build environment for the local skate3recomp research build.
call "<Visual Studio>\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set "PATH=<Visual Studio>\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;<Visual Studio>\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;<LLVM>\bin;%PATH%"
cd /d "<recomp checkout>" || exit /b 1
%*
```
`recomp_configure.bat`:
```bat
@echo off
rem GNU-style clang driver: the SDK's flags (-fno-char8_t, -ffp-model=strict, -mcmodel) are ignored by clang-cl.
call "%~dp0recomp_env.bat" cmake --preset relwithdebinfo -B out/build/clang-relwithdebinfo -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_RC_COMPILER=llvm-rc "-DSKATE3_GAME_DATA_ROOT=<extracted disc>"
exit /b %errorlevel%
```
**Use `clang`/`clang++`, not `clang-cl`.** With clang-cl every SDK file logs `unknown argument ignored in clang-cl:
'-fno-char8_t'` and `'-ffp-model=strict'` (strict floating point matters for recompiled code), and a `u8"..."`
literal fails (`char8_t`).
`recomp_build.bat` (argument = target):
```bat
@echo off
call "%~dp0recomp_env.bat" cmake --build out/build/clang-relwithdebinfo --target %1
exit /b %errorlevel%
```
Without `exit /b %errorlevel%` a failed ninja run still reports exit 0; check the log for `FAILED:` / `): error:`
either way. `SKATE3_GAME_DATA_ROOT` is read only by codegen (translating `default.xex` from your extracted disc).
Run the .bat files through PowerShell or cmd from the recomp folder; Git Bash's `cmd //c` breaks on spaces in paths.

## Build flow
1. `recomp_configure.bat` (warns `generated/sources.cmake not found` the first time; expected).
2. `recomp_build.bat generate-all > <log> 2>&1`: builds the rexglue tool and translates `default.xex` into
   `generated/` (long; run it in the background).
3. `recomp_configure.bat` again (now picks up `generated/sources.cmake`).
4. `recomp_build.bat skate3`. Never rebuild while `skate3.exe` runs (the exe is locked).
Check failures with `grep -m5 " error:\|FAILED" <log>`.

## Instrumentation
- A header-only trace writer in the SDK (`rex/audio/audio_trace.h`), active only when env `SKATE3_AUDIO_TRACE_FILE`
  names an output file; tab-separated, first field after the kind = ms since start:
  - `READ <ms> <path> <file offset> <length> <guest address>` (hook in the kernel's `NtReadFile` after the read);
  - `XMA <ms> <context> <buffer index> <guest address> <packet count>` (hook where the XMA context reads its input
    buffer address).
- Gameplay-side hooks (POST, SPLC, PLAY/GAIN/SEND/MOD, world one-shots, ambience, ...) live in the recomp's
  `src/research/hooks_*.cpp` with a shared `trace_common.h`: see skill `recomp-research` for how to write them and
  skill `recomp-scripted-runs` for the trace line formats. Rebuild after editing hooks.
- A published set of these research hooks exists as a `research-hooks` branch of a skate3recomp fork; the
  `tools/recomp-code-search/README.md` in the repo links it.

## Analysis
1. From `READ` lines on `data/audio/*.big`: which bank file range was loaded at which guest address range.
2. Each `XMA` input address -> containing loaded bank -> offset in the bank -> SNR stream index, using the scanner in
   `tools/asset_pipeline/audio_formats.py` (`scan_snr` / `splc_streams`) on the same bank (from the
   BIG via `tools/owned_game/big.py`). The index matches `audio_manifest.json` bank indices for exported banks.
3. Fallback if EA copies packets to a separate buffer: match the packet bytes against bank data.
4. Find EA's AEMS post-event function from call stacks to log event ids; name them from the `.csi` MOIR records
   (16 bytes: name offset, 16-bit id, default value).
5. Port the mechanism (skill `aems-port`), then have a person confirm by ear (skill `audio-tuning`).

## Unattended sessions
Use skill `recomp-scripted-runs` (scripted pad input, recorded routes, background mode, window screenshots) instead
of hand-marked sessions.
