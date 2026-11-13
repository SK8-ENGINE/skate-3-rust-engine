# Setup survives a dead console (#20)

Branch: `fix/setup-dead-console` (from `main` 4488651). Status: **done, in upstream main** (`d4e00bad`, integrated through `c283cc57`). Setup (Python) only; no asset
rebuild, no game behaviour change.

## Problem

Upstream issue #20 (reported by @NotTylor): setup from `default.xex` stopped in the customiser library stage with

```
File "tools\asset_pipeline\customisation_library.py", line 152, in prepare
    print('Prepared',part['slot'],len(model_data),'models',len(mat_data),'materials',flush=True)
OSError: [Errno 22] Invalid argument
```

The catalog stage before it had finished normally. The conversion itself was healthy; only a progress print failed.

## Root cause

`skate3setup.exe` inherits the game's stdout/stderr (`crates/skate-game/src/setup.rs` starts it with
`Command::new(setup)…status()` and no `Stdio` override), and the game inherits the supervisor's pipe. When that pipe or
console is gone (supervisor ended, launched from a shortcut or launcher without a console, etc.), every write to it
raises. On Windows a write to a pipe whose reader has closed raises exactly `OSError: [Errno 22] Invalid argument`
(reproduced locally with a closed `subprocess.PIPE`). The setup worker's prints are diagnostics only, but the exception
propagates through `install()` and aborts the whole conversion. Why the handle was dead for the reporter is still
unknown; the hardening holds either way.

## Change

`tools/setup.py`: `main()` first wraps `sys.stdout` and `sys.stderr` in `TolerantStream`, which forwards everything
to the real stream until a write or flush raises `OSError` (Errno 22, broken pipe, …) or `ValueError` (closed file).
From then on that stream's output is dropped silently and the setup carries on. It covers every mode of the packaged
worker: the setup window's conversion thread (the #20 path), `--task` conversion subprocesses (their stdout is a pipe
that `install.py` copies into the stage log, unaffected) and `--character-import`. A missing stream (`None`, windowed
exe) is left as it is. Files are not touched: `data\setup-error.log`, the stage logs and `setup-report.json` are
written exactly as before.

No print site in a fingerprinted file was edited. `tools/setup.py` is not part of any asset group fingerprint
(`tools/asset_pipeline/versions.py`); `customisation_library.py`, where the failing print lives, is, so it stays
untouched. `fingerprints()` gives identical hashes for all five groups before and after the change, so existing
installs don't refresh any assets.

## Files

- `tools/setup.py`: `TolerantStream`, `tolerate_dead_console()`, called at the top of `main()`.
- `tools/test_setup_dead_console.py`: new tests.

## Verification

- `py -3.13 -m unittest tools.test_setup_dead_console -v`: 5 pass.
  - `TolerantStream` swallows `OSError(EINVAL)`, `BrokenPipeError` and `ValueError` on write/flush and stops retrying
    after the first failure; a working stream is forwarded unchanged; the guard wraps each stream once.
  - End to end: a child process runs `setup.main()` in `--task` mode after the parent has closed both its stdout and
    stderr pipes; the task prints 2000 progress lines to each, then writes its output file. Exit code 0, file written.
    Control: the same task without the guard dies on its first print (non-zero exit, no output file), as in #20.
- All setup / asset-pipeline Python suites (`py -3.13 -m unittest` over `tools/test_*.py` and
  `tools/asset_pipeline/test_*.py`): 114 pass, 1 skipped (pre-existing skip). The three modules that import from
  `tools/` directly run the CI way (`discover -s tools`, as in `.github/workflows/release.yml`): release 2, update 22,
  HUD font 1, prepared HUD 4, all pass.
- Group fingerprints before/after: identical.

## Open questions

- Why the reporter's console handle was dead. Starting `support\skate3setup.exe` while the game had already exited
  would do it; a log from the reporter would tell.
- Errors stay visible: the setup window still shows the failure dialog and `data\setup-error.log`, which never
  depended on the console.
