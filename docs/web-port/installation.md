# Your installation in the browser

The browser build plays from the player's own converted installation, the same one the native
build uses. Nothing is uploaded: the page asks for a folder, reads files from this computer's
disk as the engine needs them, and keeps the settings the engine writes in the browser.

## Testing it with your installation

1. Convert your copy with the native setup, as for the Windows build. That gives a package folder
   holding `data/installation.json` and `data/installations/<id>/` (`assets/`, `maps/`,
   `settings/`).
2. Build the browser package (Linux, macOS, or Git Bash on Windows; the requirements are in
   [`tools/web-port/README.md`](../../tools/web-port/README.md)):

   ```
   tools/web-port/build.sh
   ```

   It refuses uncommitted changes (the page's source link must match the build);
   `SKATE_WEB_ALLOW_DIRTY=1` builds anyway, for local testing only.
3. Serve `target/web/dist` from `localhost` (WebGPU needs a secure context, and `localhost` is one):

   ```
   python3 -m http.server --directory target/web/dist 8080
   ```

4. Open <http://localhost:8080> in a current Chrome or Edge (WebGPU on). Choose
   **Choose installation folder** and pick the package folder, its `data` folder, or
   `data/installations/<id>`. Chrome asks to confirm when it falls back to its upload-style
   folder dialog; the files still stay on your computer.
5. The page shows what it reads, then the game. If startup stops, the page says why. Open the
   console (F12) for the engine's lines: `SKATE_WEB_STATUS` (progress), `SKATE_WEB_READ_AHEAD`
   (files startup wanted that the read-ahead rules missed, with their paths), `SKATE_WEB_READY`
   (the WebGPU adapter) and `SKATE_WEB_STOPPED` (why it stopped).

On the next visit, Chrome and Edge offer **Use “<folder>” again**, which asks to read the same
folder without picking it.

Pads come through the Gamepad API (press a button on yours once the page is open), and audio
through Web Audio: see [input-audio.md](input-audio.md).

The automated check, `node tools/web-port/smoke.mjs`, cannot use real game data, so it uses a
synthetic installation instead (below).

## How it works

```
page (web/installation.js)           engine (crates/skate-fs, crates/skate-game/src/web.rs)
──────────────────────────           ───────────────────────────────────────────────────────
choose folder → list Files           (no file is read yet; a File is a reference to disk)
find data/installations/<id>
globalThis.skateInstallation   ───►  skate_fs::mount: the file list becomes a tree at /installation
                                     read ahead: settings, then the start map
prefetch(paths) reads them     ◄───  skate_fs::prefetch(...).await
                                     crate::run(): the native startup, unchanged, reading through
read(path) from what it holds  ◄───  skate_fs::read (synchronous)
                                     a file that was not read ahead: startup fails cleanly, the
                                     file is read, startup runs again
fetch(path) reads on demand    ◄───  the asset server (models, textures), without blocking
saved(path, bytes) → IndexedDB ◄───  settings written by the engine
```

- **`skate-fs`** is the engine's file API. Natively it re-exports `std::fs` (and
  `std::env::temp_dir`), so native behaviour is unchanged. In the browser it serves the mounted
  installation from memory: synchronous reads come from what the page has read ahead, writes stay in
  memory for the session and are handed to the page, and paths are absolute under `/installation`.
- **Read-ahead.** Startup reads synchronously, but a page can only read the disk asynchronously,
  and an installation is far too big to read whole (1.5–3 GB, mostly maps, audio and the
  customiser). So before startup, `web.rs` reads ahead:
  - `settings/`
  - under `assets/private`: the JSON files, menu movies, HUD and particles
  - the stock files startup reads: input config, state and camera graphs, the two animation
    banks, gesture patterns, camera shakes and CAC recipes
  - the audio, except the ambience beds and decoded speech, which are read as they play
    ([input-audio.md](input-audio.md))
  - the start map named by `settings/default-map.json`, with its sidecars, sky and lighting

  The customiser's models and textures, and the character, go through Bevy's asset server, which
  reads them without blocking.
- **Misses.** A startup read of a file that was not read ahead fails with `WouldBlock` and is
  recorded. Startup then fails the way it fails natively, so the browser reads the recorded files
  and runs startup again, up to 12 times. Reads after the game starts (Startup systems and
  frames) cannot retry: anything they need must match a read-ahead rule. Any such miss is logged
  as `SKATE_WEB_READ_AHEAD`.
- **Map changes.** Natively the map loader runs on worker threads. The browser has none, so it
  reads the new map's files ahead, then runs the same loader on the main thread between frames.
  The screen holds on the loading overlay for that time. The map left behind is released.
- **Settings** persist per installation (keyed by its `<id>` folder name) in the browser's
  IndexedDB, and override the files on disk on later visits. A folder handle chosen through
  Chrome's folder picker is remembered the same way.

## What the browser build leaves out (for now)

| | Why | Where it comes back |
|---|---|---|
| Multiplayer | No UDP or Steam relay in a browser | Milestone 5 (WebSocket relay) |
| Lua mods, imported custom characters | No Lua build for wasm32; the character library lives in `%LOCALAPPDATA%` | — |
| Setup and the installation check | The page cannot run the setup tool; it only checks for `assets/private/game.json`. Refresh an outdated installation natively | — |
| FPS cap | The browser paces frames to the display | — |

## The synthetic installation

`tools/web-port/smoke.mjs` builds one in the native package layout from placeholders and the
repository's own procedural `maps/format-demo.skate`. It holds no game data, so the engine
goes as far as anything but a real converted installation can:
1. mount it and read ahead
2. load the settings
3. follow `default-map.json` to the demo map, which it parses and fingerprints
4. stop at the placeholder `input.cfg`.

The test withholds that file from the first read-ahead. Startup must therefore notice the miss,
read the file and start again. Two more pages check the folder picker with no folder, and a folder
that holds no installation.
