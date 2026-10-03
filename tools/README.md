# Tools

The top-level scripts here are the repository's own tooling: setup and asset preparation (`setup.py`,
`prepare_assets.py`, `asset_pipeline/`, `owned_game/`), the updater, release and build helpers, and their
tests. They are documented where they are used (the main README and the scripts' own help).

The folders below are standalone helper tools for development and research. Each has a `README.md`
with what it does, its inputs, usage, example output and requirements. They read your own copy of the
game; none of them contain game code or data. Default work folders are under `.local/` (gitignored).

| Folder | What it is for |
|---|---|
| [`regression-checks/`](regression-checks/README.md) | Map validation and collision counts, baked spawns, customiser outputs against your own baselines; a muted crash smoke test per map. |
| [`setup-equivalence/`](setup-equivalence/README.md) | Prove a faster setup path gives identical bytes (district stream loader, native RefPack DLL). |
| [`collision-inspect/`](collision-inspect/README.md) | Extract a district's collision for analysis: triangles, surfaces at a point, surface ids and flags, surfaceless meshes. |
| [`world-stream-inspect/`](world-stream-inspect/README.md) | List / extract `.big` archives; survey the RW4 arenas of the district simulation streams; dump named trigger volumes. |
| [`audio-file-inspect/`](audio-file-inspect/README.md) | Readers for the audio formats: ABKC banks and MOIR projects, SPLC banks, `.ems` emitter files, `.grain` data; decode bank samples. |
| [`vault-inspect/`](vault-inspect/README.md) | Look up fields in the attribute database (converted skater collections) and class layouts in the schema. |
| [`audio-e2e/`](audio-e2e/README.md) | Scripted scenarios for the headless audio render and analysis of the renders (diffs, levels, voices, bus share). |
| [`audio-bench/`](audio-bench/README.md) | Audio performance: hashed e2e bench runs, timing summaries, emitter bank memory. |
| [`recomp-trace/`](recomp-trace/README.md) | **For use with the Skate 3 recomp's research hooks** ([`research-hooks` branch](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)): read and analyse trace sessions. Reference only; you build the recomp and set up the paths yourself. |
| [`recomp-code-search/`](recomp-code-search/README.md) | **For use with the Skate 3 recomp's research hooks** ([`research-hooks` branch](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)): search the recompiled sources and the memory image. Reference only; you build the recomp and set up the paths yourself. |

See `docs/hails-additions/14-published-tools.md` for the background.
