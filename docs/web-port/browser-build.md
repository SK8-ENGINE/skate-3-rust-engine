# Browser build

`skate-game` compiles for `wasm32-unknown-unknown` and renders through WebGPU. In a browser the
page asks for the player's own converted installation, the engine starts from it and renders on
the page's canvas ([installation.md](installation.md), which also says how to test it with yours).
Everything below is behind `cfg(target_arch = "wasm32")`, a wasm-only dependency or `skate-fs`,
which is `std::fs` natively; the native build is unchanged.

## Building and checking

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version =0.2.127 --locked
tools/web-port/build.sh            # -> target/web/dist: index.html, installation.js, skate3rust.js, skate3rust_bg.wasm
node tools/web-port/smoke.mjs      # the picker and a synthetic installation in headless Chromium with WebGPU
```

`build.sh` uses the `web` cargo profile (release code without debug info) and stamps the commit and
the source repository (this checkout's `origin`, or `SKATE_SOURCE_URL`) into the loader page's
GPL source link. It refuses a tree with uncommitted changes, since the link would not match the
build; `SKATE_WEB_ALLOW_DIRTY=1` is for local testing only.

`smoke.mjs` serves the package on `localhost`, a secure context, so WebGPU is exposed, and opens
three pages:
- the folder picker
- a folder with no installation
- a synthetic installation (no game data; [installation.md](installation.md#the-synthetic-installation)),
  which startup must load up to its placeholder input config, retrying once for a file withheld
  from the read-ahead.

It fails on a panic, a console error or a page error, and writes a screenshot to
`target/web/smoke.png`.

Results at milestone 6 (Rust 1.97.0; Playwright's Chromium with SwiftShader WebGPU):

| | |
|---|---|
| `web` profile build of the engine (4 cores) | 9 min 19 s with dependencies cold (milestone 2); about 3 min after a change to the game |
| `skate3rust_bg.wasm` | 132.6 MB raw, **22.1 MB gzip** (what a static host sends) |
| `smoke.mjs` | picker, no-installation and synthetic-installation pages pass with no console error, panic or page error. The synthetic installation reaches its placeholder input config after one read-ahead retry. The fake standard pad reaches slot 0 and the unmapped one is refused. After a 400 ms stall, 22 of 22 audio buffers are scheduled on time (15 of 30 were late with unpatched cpal) |
| `installation.test.mjs` | 4 passed |
| Native `skate-game` tests | 494 passed (the 490 from before the port plus 4 new input tests), 183 ignored; the setup fingerprint test fails as it did before the port |
| Native `skate-fs`, `skate-data`, `skate-net`, `skate-audio` tests | all pass (6, 44, 40 and 253) |
| Native `skate-mods` tests | 105 passed; `skyline_physics` fails as before (it needs `sdk/examples/skyline/skyline.glb`, which `.gitignore` keeps out of every checkout) |

What the smoke test cannot show:
- **Pixels.** Headless Chromium here composites no WebGPU canvas into screenshots or `drawImage`
  copies (even a canvas cleared to red reads back blank), so the stop screen's look was not
  checked. Its pipelines compiled and ran without a WebGPU error, which the console check would
  have caught.
- **The game itself.** Only a real converted installation gets past the stock loaders, and none
  can be used here. Playing in a browser has to be checked with your own
  ([installation.md](installation.md#testing-it-with-your-installation)).

## Hosting

The package is static files, so any static host works. WebGPU only runs on HTTPS pages (or
`localhost`). GitHub Pages fits (1 GB per site) when deployed from Actions; a 130 MB file is over
git's 100 MB push limit, so it is not committed. Cloudflare Pages caps a single file at 25 MiB:
put `skate3rust_bg.wasm` on R2 or use another host. Shrinking the module (`wasm-opt`, size-tuned
codegen) belongs with the server and hosting work (milestone 4). Never upload a converted
installation anywhere public.

## What is gated, and why

| Area | Browser build | Native |
|---|---|---|
| Rendering | `Backends::BROWSER_WEBGPU` on the page's `#skate3` canvas, sized to the page; Bevy's `webgpu` feature (wasm-only dependency) | Vulkan, 1280×800 window |
| Logging | Bevy's `LogPlugin` → browser console | `profiling.rs` owns tracing (Chrome trace file); `LogPlugin` off |
| Process-level startup | none: no updater recovery, setup tool modes, crash supervisor or trace file | unchanged |
| Installation | chosen on the page and mounted at `/installation` by `skate-fs`; startup reads ahead, then runs as natively ([installation.md](installation.md)) | `--assets`, `SKATE3_ASSETS`, `data/installation.json` |
| File access | `skate-fs`: the mounted installation, writes in memory and IndexedDB | `skate-fs` re-exports `std::fs` |
| Clocks | `bevy::platform::time::Instant` (`web-time`), `web-time`'s `SystemTime` | the same types are `std::time`'s |
| Map changes | files read ahead, then the loader runs on the main thread | worker threads |
| Game audio | Web Audio through Bevy/rodio/cpal; the audio core read ahead, beds and speech read as they play ([input-audio.md](input-audio.md)) | unchanged |
| Controllers | the Gamepad API's standard layout in the XInput shape ([input-audio.md](input-audio.md)) | SDL3 (XInput fallback on Windows) |
| Lua mods | none: `mlua`'s vendored C Lua does not target wasm32-unknown-unknown. `skate_mods::Vm` is uninhabited there and package validation says why | unchanged |
| Asset sources | the default source reads the mounted installation without blocking; `mods`, `characters`, `online-characters` are not registered | `FileAssetReader` sources |
| Multiplayer | UDP sockets and the Steam relay helper fail to open (browsers have neither); the WebSocket relay is milestone 5 | unchanged |

## Runtime safety

Milestone 3 gave the browser a path for each of these:
- `Instant::now` and `SystemTime::now`, which panic on wasm32-unknown-unknown.
- `std::env::temp_dir`, which panics there too; `skate_fs::temp_dir` is in memory.
- Thread sleeps (the FPS cap).
- The map loader's threads.
- Path-based loading.

Still open:
- `skate-dynamics` wraps convex decomposition in `catch_unwind`, which cannot catch on wasm
  (panics abort), so bad input must be rejected before it.
- The audio bank prefetch thread fails to start in the browser and falls back to loading on the
  game thread (2.6–5.4 ms per bank, from files read ahead).
- The frame and audio-state logs (`SKATE_FRAME_LOG`, `SKATE_AUDIO_STATE_LOG`) write from threads,
  but environment variables are never set in a browser, so they never start there.
- Multiplayer transports (milestone 5).
