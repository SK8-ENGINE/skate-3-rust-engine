# Audio file inspection

Readers for Skate 3's audio formats, run against your own disc. They check the layouts the native
audio runtime (`crates/skate-audio`) and the audio setup (`tools/asset_pipeline/audio_export.py`) rely
on, and let you look inside a bank, an emitter file or the grain data. All read-only, all headless.

| Script | What it does |
|---|---|
| `aems_survey.py` | Surveys every ABKC bank (`.abk`) and MOIR project (`.csi`) in `audiofiles.big`: header invariants, records per bank, capacities, program opcode census, export kinds, voice objects, curve types, and a failure list. `--dump BANK.abk` prints one bank's records, instance layout and program. |
| `bank_layout_check.py` | Checks that each bank's S10A slot order matches the stream scan setup uses (so exported WAV index = slot index), and whether the rebase / interface lists lie after the sample data (they do, so the runtime needs the whole `.abk`). `--extract DIR` also writes every `.abk` / `.csi` (plus `csi_order.txt`) for the tools below. |
| `decode_bank_samples.py` | Decodes chosen banks' samples to WAVs the way setup does (`<out>/<stem>/NNNN.wav`, slot order), for banks the install does not export. Uses vgmstream-cli. |
| `multichannel_census.py` | Which sample-group entries of the banks' player objects use multichannel (2/4/6-channel) samples, and their azimuth bytes. |
| `splc_fields.py` | Census of SPLC (`.bnk`) record, group and member fields: value ranges per 4-byte offset (as f32 and u32). With a bank and record id, dumps that record raw. |
| `ems_dump.py` | Dumps the world emitter files (`.ems`): records per file, the most used sounds (64-bit name ids resolved against the archive's member names), and with `--records` every record's position, extent, scalars and gains. |
| `grain_survey.py` | Surveys the granular rolling-bed data: per `.grain` member the header, seek table and EAAC stream header; per grain class the vault tuning (from the install's converted database). `--rms` adds a loudness / spectral-centroid profile per member. Writes JSON. |

## Inputs

- Your extracted disc: `--disc DIR` (the folder holding `data/`), default `$SKATE3_DISC` or `.local/skate3-disc`.
- Extracted banks for `decode_bank_samples.py` / `multichannel_census.py`: run
  `bank_layout_check.py --extract .local/audio-file-inspect/banks` first (the default location).
- `.bnk` files for `splc_fields.py`: extract them with
  `tools/world-stream-inspect/big_list.py <disc>/data/audio/audiofiles.big "\.bnk$" --out DIR`.
- `grain_survey.py` vault part: `assets/private/stock/skater-collections.json` from setup (`--vault`).

Outputs go to `.local/audio-file-inspect/` (gitignored). They are decoded game data: keep them local,
never commit them.

## Usage

```
py -3.13 tools/audio-file-inspect/aems_survey.py
py -3.13 tools/audio-file-inspect/aems_survey.py --dump SomeBank.abk
py -3.13 tools/audio-file-inspect/bank_layout_check.py --extract .local/audio-file-inspect/banks
py -3.13 tools/audio-file-inspect/decode_bank_samples.py SomeBank OtherBank
py -3.13 tools/audio-file-inspect/multichannel_census.py
py -3.13 tools/audio-file-inspect/splc_fields.py .local/bnk
py -3.13 tools/audio-file-inspect/ems_dump.py sfx_ --records
py -3.13 tools/audio-file-inspect/grain_survey.py --rms
```

## Example output

`bank_layout_check.py` (made-up counts):

```
120 banks; slot order mismatches 0; banks whose rebase/interface lists lie after the sample data (so the whole file is needed, not just the resident part): 120
```

`ems_dump.py` (made-up names):

```
data/audio/sfx_example.ems: 42 records; top: [('water_loop', 12), ('?0123456789ABCDEF', 3)]
records 42, sound ids resolved 39
```

## Requirements

- Python 3.13; `numpy` only for `grain_survey.py --rms`.
- The repository's setup modules (`tools/owned_game`, `tools/asset_pipeline/audio_formats.py`,
  `audio_export.py`).
- vgmstream-cli for `decode_bank_samples.py` and `grain_survey.py --rms`: setup downloads it to
  `data/tools/vgmstream-cli/`; or pass `--vgmstream` / set `VGMSTREAM`.
