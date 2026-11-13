# Audio specs

Behavioural specifications behind the native audio port ([doc 11](../11-audio.md)) and the world audio
hook-in ([doc 15](../15-world-audio.md)). Code comments in `crates/skate-audio` and
`crates/skate-game/src/game_audio` cite them as `audio-specs/<name>.md §n`; the section numbers are kept
stable for that reason.

## What these are

- Written in our own words from reading the Skate 3 TU 3.0.3.0 recompilation, the disc data and the
  attribute database, then checked against traces of the running recompilation. Function addresses
  (`sub_8xxxxxxx`), object offsets and image constants are given as references so a reader can find the same
  code; no game code or game data is copied here.
- Measured values (levels, rates, timings, counts) come from the recompilation unless a line says otherwise.
  The recompilation is not the console: its frame rate, timing and threading differ, so its program logic is
  the strong evidence and its timings are weaker.
- Upstream PR #4 and PR #1 (@andrewnakas) and the other prior work were read for behaviour only; see
  [`rwaudio-prior-work.md`](rwaudio-prior-work.md) and the credit tables in the specs.
- These are working notes. Tags such as **[R]** (our reading of the code), **[IMG]** (image constant),
  **[TR]** / **[T]** (trace-checked), **[I]** (inference) and **UNCERTAIN** say how firm each statement is.
  Later findings are added as dated sections; superseded text is marked rather than removed.
- Session names (`all_20261002_164620`, `state_20261003_084712.tsv`, …) are labels of recordings made
  during the work; the recordings themselves are local and not published. Scripts named as "local" are not
  published either; the published ones are under [`tools/`](../../../tools/README.md) (doc 14).

## Index

| Spec | Covers |
|---|---|
| [`aems-reference.md`](aems-reference.md) | Overview: ABKC bank / program layout, evaluator tick, opcode list, sources, corrections; index of the specs below |
| [`aems-evaluator-spec.md`](aems-evaluator-spec.md) | AEMS patch-program evaluator: bank and MOIR layouts, runtime, all 40 opcodes, timing, RNG, test plan |
| [`aems-voice-graph-spec.md`](aems-voice-graph-spec.md) | Voice graph and output mix: SndPlayer1, resample, filters, gain, pan, sends, buses, output stage |
| [`aems-env-bus-spec.md`](aems-env-bus-spec.md) | Environment (reverb) bus: send routing, reverb / delay sub-mixes, presets, zones |
| [`aems-eqchain-buses-spec.md`](aems-eqchain-buses-spec.md) | eEQChain material buses, EQ jitter, LFO (low-frequency oscillator) task, owner send buses |
| [`aems-bus-leftovers-spec.md`](aems-bus-leftovers-spec.md) | FlangeSub returns, sense_of_speed FXWET0, Splice Send A, Collision SubMix, reverb zones |
| [`mixmap-spec.md`](mixmap-spec.md) | MixMap (`MixMapSK8.mxb`): format, evaluation, ambience / emitter outputs, ducking, validation |
| [`grain-player-spec.md`](grain-player-spec.md) | Granular rolling bed: `grains.big`, surface → grain map, GrainPlayer pick / scheduler / fades, chains |
| [`aems-grain-chain-spec.md`](aems-grain-chain-spec.md) | Board grain bus chain (`sub_824C8878`): FSS (frequency shift), shelf, clip, sends; listening-report evidence |
| [`aems-board-layers-spec.md`](aems-board-layers-spec.md) | SFXObj_SkateBoard rolling layers: Class_rolling, rattles, board slide |
| [`aems-tricks-treatment-spec.md`](aems-tricks-treatment-spec.md) | Tricks component (Class_Flips, cloth_trick) and Class_Treatment |
| [`aems-offboard-clothing-spec.md`](aems-offboard-clothing-spec.md) | Off-board footsteps, clothing, hands-on-deck |
| [`ragdoll-contact-spec.md`](ragdoll-contact-spec.md) | Ragdoll contact response and the bail-sound gap (research) |
| [`ems-emitters-re.md`](ems-emitters-re.md) | World emitters (`.ems`), random one-shot sets, zone ambience beds |
| [`npc-livingworld-re.md`](npc-livingworld-re.md) | The living world: census, traffic, pedestrians, security, AI skaters, NPC speech (background for the world audio) |
| [`world-audio-hookin-spec.md`](world-audio-hookin-spec.md) | Design of the engine-facing world audio API and mod surface (doc 15) |
| [`world-traffic-audio.md`](world-traffic-audio.md) | Traffic audio: retail mechanism, port state, hook points |
| [`world-ped-audio.md`](world-ped-audio.md) | Pedestrian audio: retail mechanism, port state, hook points |
| [`world-speech.md`](world-speech.md) | Pedestrian speech: data, mechanism, port state |
| [`world-npc-skater-audio.md`](world-npc-skater-audio.md) | NPC (AI) skaters' board sounds |
| [`rwaudio-prior-work.md`](rwaudio-prior-work.md) | Prior work on RenderWare Audio and Snd9 AEMS, licences, applicability |

## Credits

[skate3recomp](https://github.com/mchughalex/skate3recomp) by @mchughalex, built on the
[rexglue SDK](https://github.com/rexglue/rexglue-sdk) and [Xenia](https://github.com/xenia-project/xenia)'s
Xbox 360 research; upstream PR #4 and PR #1 by @andrewnakas; sk8Audio (andrewnakas/skate3-audio);
BurnoutDecomp's b5-decomp and BP-Decomp_Workflow; dbalatoni13/nfsmw; mitsevox/tw2004;
[vgmstream](https://github.com/vgmstream/vgmstream). Details and licences per source are in each spec.
