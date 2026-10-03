# Audio end-to-end renders

The engine can render its native player audio headless, frame by frame, from a scripted scenario: the
test `game_audio::e2e::e2e_render` in `crates/skate-game` (it is `#[ignore]`d, so it only runs when asked).
These scripts write the scenarios and analyse the renders, so an audio change can be checked by
numbers (and proved byte-identical when it should be) without playing.

| Script | What it does |
|---|---|
| `scenarios.py` | Writes the scripted scenarios (one TSV per scenario, one row per 60 Hz frame: speed, turn, wheels down, surface tag, air, grind, brake, manual, push, …). `--from-log` cuts a real play session's audio state log into a scenario instead. |
| `render_diff.py` | Compares two folders of renders: per scenario bit-identical or not, largest sample difference, level of each and the level change. |
| `compare.py` | Compares two renders of the same scenario in one folder (`<name>.ours.f32` against `<name>.<other>.f32`) per segment: level, octave bands, spectral centroid, pitch, onsets, envelope lag and correlation. `--json` writes the report. |
| `frame_levels.py` | Per frame: the scenario state, the output level (dBFS) and the summed gain of each voice group (grain bed, wheels, one-shots, each bank). |
| `voices_summary.py` | Per frame: summed voice gain by bank, from `<name>.ours.voices.tsv`. |
| `bank_rate.py` | Voice starts per second and summed gain of one bank, per surface tag and seam pattern, across one or more render folders. |
| `wet_level.py` | The bus (reverb / effects) contribution: RMS of (wet render − dry render) against the dry render, over the rolling frames. |

## Inputs

- Scenario TSVs (from `scenarios.py`) in a folder, default `.local/audio-e2e/`.
- The renders the test writes next to them: `<name>.ours.f32` (raw float32, 6 channels L R C LFE Ls Rs,
  48 kHz, one settle second first) and `<name>.ours.voices.tsv`.
- An installation with the native audio data (setup group `audio`).
- Real-play logs for `--from-log`: run the game with `SKATE_AUDIO_STATE_LOG=<path>`.

## Usage

```
py -3.13 tools/audio-e2e/scenarios.py .local/audio-e2e
set E2E_DIR=<repo>\.local\audio-e2e
cargo test -p skate-game --release --bin skate3rust --locked -- --ignored e2e_render --nocapture

py -3.13 tools/audio-e2e/scenarios.py --from-log my_session.tsv --cut 12-40 roll_session .local/audio-e2e
py -3.13 tools/audio-e2e/render_diff.py .local/audio-e2e-before .local/audio-e2e
py -3.13 tools/audio-e2e/compare.py .local/audio-e2e --a ours --b other --only roll20
py -3.13 tools/audio-e2e/frame_levels.py .local/audio-e2e roll20 0 600 30
py -3.13 tools/audio-e2e/wet_level.py .local/e2e-dry .local/e2e-wet roll20
```

`E2E_ONLY=a,b` limits the test to some scenarios. To compare against another build or another audio
stack, render into a second folder and use `render_diff.py`, or rename its renders to
`<name>.<label>.f32` in the same folder and use `compare.py --b <label>`.

## Example output

`render_diff.py` (made-up values):

```
scenario        same     max|d|   A dBFS   B dBFS  B-A dB
roll20          True          0   -24.31   -24.31   +0.00
grind_metal    False    0.00012   -21.80   -21.75   +0.05
```

## Requirements

Python 3.13; `numpy` for `compare.py` and `render_diff.py`. Rust toolchain for the render test.
