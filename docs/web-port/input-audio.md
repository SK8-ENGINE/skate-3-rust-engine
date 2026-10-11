# Controllers and audio in the browser

Milestone 6: the browser build reads pads through the Gamepad API and plays the game's audio
through Web Audio. Native input and audio are unchanged: SDL3/XInput and the same audio code
paths, built from the same sources.

## Controllers

`input/platform.rs` re-expresses every pad as the XInput-shaped `XboxState` that
`xbox::convert` consumes. Natively an SDL thread fills four slots. In the browser,
`navigator.getGamepads()` fills the same slots each time a slot is polled.

| Gamepad API "standard" layout | XInput |
|---|---|
| buttons 0–3 (bottom, right, left, top face) | A, B, X, Y (by position, like SDL's South/East/West/North) |
| 4, 5 | left, right shoulder |
| 6, 7 (analog, 0–1) | left, right trigger byte (0–255) |
| 8, 9 | back, start |
| 10, 11 | left, right stick click |
| 12–15 | d-pad up, down, left, right |
| axes 0/1, 2/3 (−1–1, Y down) | left and right stick (−32768–32767, Y up) |

- **Slots.** Pads keep their slot while connected. A new pad takes the slot of its browser index
  if that is free, else the first free one. The packet number advances when the state changes,
  as with SDL.
- **Only the standard layout.** A pad the browser does not map to the standard layout is
  reported once in the console and not used: its sticks could be anywhere. Chrome, Edge and
  Firefox map common Xbox, PlayStation and Switch Pro pads. The guide button has no XInput bit,
  and the standard layout has no paddles.
- **Identity.** The id string gives the name, and in Chrome and Firefox the USB vendor/product,
  which the model table turns into a family, e.g. "DualSense [PlayStation 5] 054c:0ce6, Gamepad API".
- **Before a pad appears,** browsers want one of its buttons pressed while the page is open.
  The stop screen lists the pads the browser exposes, so a pad can be checked even before the
  game starts.
- The game sends no rumble.

## Audio

Output needs no new code. Bevy's audio (rodio on cpal) already plays through Web Audio in a
browser, as `AudioPlayer` voices and the native AEMS runtime's `NativeStream`. What needed work
is loading: the audio code reads files synchronously, and a page can only read the player's disk
asynchronously.

- **Read ahead at startup:** everything under `assets/private/audio` except the ambience beds and
  decoded speech. That is the AEMS projects and banks, MixMap, grains, wheels and the speech
  indexes, about 120 MB. The runtime then starts and loads banks exactly as it does natively. The
  bank prefetch thread cannot start in a browser, so banks decode on the game thread when an
  emitter first needs them, at 2.6–5.4 ms per bank (`docs/hails-additions/11-audio.md`).
- **Read on demand:** ambience beds (about 25 MB each, about 550 MB in all) and speech takes.
  In the browser, a read of one that is not in yet fails with `WouldBlock` rather than a real
  error:
  - `Library::clip` then reads the file in the background instead of marking it failed.
  - The ambience system starts a pending bed once it is in, if its zone still wants it.
  - A speech line whose take is not in yet is silent; later lines play it.
  - The page drops each such file once the engine has its bytes.
- **Autoplay.** Browsers may hold audio until the player interacts. The page resumes any
  suspended audio context on the next click or key. Choosing the installation folder is already
  a click.
- **Stalls.** The browser has no audio thread: cpal fills 46 ms buffers from the page's main
  thread, about 100 ms ahead. A main-thread stall longer than that (a map load) leaves a gap.
  Unpatched cpal 0.15.3 then also played its two buffers overlapped until it caught up: in the
  headless test, 15 of 30 buffers after a 400 ms stall were scheduled in the past. The vendored
  cpal (`vendor/README.md`) restarts its clock instead. With it, 22 of 22 buffers after the same
  stall were scheduled on time.
- **Cost:** the native runtime renders 48 kHz blocks on the main thread at about native speed,
  roughly 2.5 ms per 46 ms buffer. rodio resamples it to cpal's 44.1 kHz.

## Checked here, and what needs your installation

`tools/web-port/smoke.mjs` plugs two fake pads into the synthetic installation's page:
- the standard Xbox 360 pad must reach slot 0 through the Gamepad API;
- the unmapped one must be refused;
- after a 400 ms stall, no Web Audio buffer may be scheduled in the past.

The new mapping and identity code has native unit tests (`input::`).

What no test here can do is play: only a real converted installation gets past the stock
loaders. To check with yours:
1. Pick it as in [installation.md](installation.md).
2. Press a button on your pad.
3. Skate.

In the console, look for:
- `Controller 0: gamepad …` and `identified as …` when the pad appears;
- `AUDIO_AMBIENCE` lines as you move between zones;
- `Game audio` warnings for anything that failed to load.

## Performance

Find slow spots with the in-game frame stats (graphics menu) and Chrome's Performance panel
(F12 → Performance → record a few seconds of skating). Measured and fixed here: audio after a
main-thread stall (above). Known costs that need a real session to weigh:
- **Map changes** run the loader on the main thread, so the screen and the audio hold for its
  length.
- **Banks** decode on the game thread (above).
- **Memory.** The read-ahead files stay in the page for the session: the audio core (about
  120 MB) and the start map. The decoded copies live in the engine's memory, which wasm32 caps
  at 4 GB.
