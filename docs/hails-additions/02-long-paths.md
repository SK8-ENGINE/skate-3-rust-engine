# 2. Windows long paths during setup

## Problem

After a successful setup, two pro skaters were unavailable (Brayden
Szafranski, Deerman of Dark Woods) with errors like:

```
[Errno 2] No such file or directory: '...\data\installations\<id>\assets\private\customisation\sets\<id>\roster-work\source\data\content\marquee\model\deerman_of_darkwoods\Rostral\0x2c7f38110008014a.rx2'
```

## Root cause

Windows' 260-character `MAX_PATH` limit. The roster work paths above are 260
and 261 characters long when the repository sits in a moderately deep folder
(`F:\VS-Code\Github Repos\skate-3-rust-engine`). Long paths were disabled on the
machine (`HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled = 0`).
`python.exe` and PyInstaller-built executables are long-path aware, so enabling
the OS setting is sufficient; no code paths need changing.

## Change

`tools/setup.py`:

- `long_paths_enabled()` reads the registry value.
- `enable_long_paths()`: if long paths are off, requests elevation once
  (`Start-Process reg.exe -Verb RunAs`) to set `LongPathsEnabled = 1`.
  Declining the UAC prompt keeps setup running; only the optional characters
  with over-long paths are then reported unavailable, as before.
- `restart()`: Windows reads the setting once per process, so after enabling
  it setup relaunches itself with the same arguments and returns the child's
  exit code. A frozen (PyInstaller onefile) build sets
  `PYINSTALLER_RESET_ENVIRONMENT=1` so the child unpacks as an independent
  instance.
- `main()` calls `enable_long_paths()` right after argument parsing, before
  the Tk window.

## Verification

- Enabling long paths and re-preparing only the two affected characters
  (`native_roster.prepare(..., only=[...])`) produced both successfully; the
  roster went from 39 to 41 characters, `optional_content.summary` reported 0
  warnings, and `skate3rust --check-assets` passed.
- The new helpers were exercised with the registry and `subprocess` stubbed:
  the "already enabled" path does nothing; the "disabled" path builds a valid
  PowerShell command (checked with the PowerShell tokenizer); `restart()`
  forwards the original arguments.
- **Tested end to end (2026-10-02, the user):** the real UAC prompt with long
  paths disabled, and the frozen `skate3setup.exe` relaunch.

## Notes for upstream

- An alternative is to shorten paths (for example a shorter roster work
  directory name) or use `\\?\` prefixes in the roster code; enabling the OS
  setting was chosen because it needs no changes to the conversion code.
- The prompt appears before the setup window without explanation; a short
  message first might be friendlier.
