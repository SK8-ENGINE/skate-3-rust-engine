# Crash report keeps the first panic

Branch: `fix/crash-report-first-panic` (from `main` 4488651). Status: **done, in upstream main** (`6ed48ca3`, integrated through `c283cc57`). Diagnostics only
(`crates/skate-game/src/crash_report.rs`); no gameplay or rendering change.

## Problem

Upstream issue #33 (reported by @terraceceo): the game crashed before the menu on an Intel UHD machine. The crash report
listed only follow-on errors ("… is invalid", i.e. a resource whose creation had failed earlier). The original error
(typically out of memory, device lost, or a size above an adapter limit) was no longer in the report, so the crash
couldn't be diagnosed from it.

## Root cause

The supervisor keeps `REPORT_PANIC` / `REPORT_NATIVE` lines in a pure last-128 ring. One panic is its message, its
location and up to 120 backtrace lines, so a single follow-on panic, or a run of them on other threads, pushes the first
error out. Two things made it worse:
- Release builds ship without symbols, so most backtrace lines are `N: <unknown>`. They carry no information and used
  up most of the 128 lines.
- A line longer than 4096 bytes was replaced by `[oversized line omitted]`. wgpu validation errors can be that long,
  so the most useful line could be dropped completely.

## Change

`Capture` (the supervisor's bounded log store):
- **First panic pinned.** The first `REPORT_PANIC` line (the panic message) and the line right after it, if it is the
  hook's `REPORT_PANIC at <file>:<line>:<col>` location, are kept in `first_panic` and never evicted. The report shows
  them in a new "First panic (pinned)" section ahead of the existing last-128 "Panic, native exception and stack"
  section, which is unchanged apart from the filtering below.
- **`<unknown>` frames dropped.** Backtrace lines of the form `N: <unknown>` are skipped by the panic hook before its
  120-line budget, so real frames get the room, and again in `Capture` (any source). Frames with a symbol name
  stay.
- **Long lines truncated, not dropped.** The pipe reader still buffers at most `LINE_LIMIT` (4096) bytes per line, and
  now hands the kept start to `Capture::record` with a `truncated` flag instead of swapping it for a placeholder.
  `sanitize` cuts lines over `LINE_LIMIT` characters the same way. A cut line ends in ` [line truncated]` and stays
  within `LINE_LIMIT`.
- The report header describes the new panic retention.

Privacy is unchanged: the privacy filters (sensitive keywords, absolute paths) run on the kept part exactly as before,
and the discarded tail is never stored. Placeholder lines (`[… omitted]`) get no truncation marker.

## Files

- `crates/skate-game/src/crash_report.rs`

## Verification

- `cargo test -p skate-game --release --bin skate3rust --locked -- crash_report`: 7 pass. 4 are new:
  - `first_panic_survives_a_flood_of_later_panics`: a first panic + location, then 1000 later panics. The ring holds
    128 lines without the first error, the pinned section still has the message and `render.rs:10:5`, and the latest
    panic is still in the ring.
  - `first_panic_without_location_pins_only_the_message`: a backtrace line after the message isn't mistaken for the
    location. An empty capture prints "Unavailable / not recorded" for the pinned section.
  - `unknown_backtrace_frames_are_dropped`: 200 `<unknown>` frames are kept neither in the panic ring nor in the logs,
    and don't appear in the report. Symbolised frames (`7: skate3rust::main`) stay, and lines that only mention
    `<unknown>` are not filtered.
  - `long_lines_are_truncated_with_a_marker`: both paths (an over-long line through `sanitize`, a reader-cut line
    with the flag) keep their start plus ` [line truncated]` and stay within `LINE_LIMIT`. A line exactly at the limit
    is untouched, and privacy placeholders are not marked.
- `cargo test -p skate-game --release --locked --no-fail-fast` (own target dir `.local/crashreport-target`): 313
  passed, 2 failed, 101 ignored. Both failures are known and pre-existing on `main`
  (`pipelines_accept_valid_group_outputs_when_fingerprint_changes`, `sky_shader_validates`).

## Open questions

- Only the first panic is pinned. A native exception (`REPORT_NATIVE`) still uses the ring; it is the last thing
  printed before the process dies, so it isn't pushed out.
- Errors logged before the first panic (not panics themselves) still live only in the 256-line log ring. For #33 a
  full log (`skate3rust.exe > log.txt 2>&1`) is still the better evidence.
- Not yet seen on a real crash. The supervisor path is shared by every crash, so a forced-panic smoke test of a release build
  is worth doing before a PR.
