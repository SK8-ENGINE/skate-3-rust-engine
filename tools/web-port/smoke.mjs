// Boots the browser package in headless Chromium with WebGPU and checks the installation path end
// to end. It serves target/web/dist on localhost (a secure context, so WebGPU is exposed) and runs
// three pages:
//
//   picker     the page offers the folder picker and loads no engine; no console error
//   not-ours   a folder with no installation in it is refused on the page
//   synthetic  a synthetic installation in the native package layout, holding only the repo's own
//              procedural maps/format-demo.skate and placeholder files (no game data). The engine
//              mounts it, reads ahead, loads settings and the start map (default-map.json → the
//              demo map, parsed and fingerprinted), then stops at the placeholder input.cfg, which
//              is as far as anything but a real converted installation goes. One read-ahead file
//              is withheld on purpose, so startup must notice the miss, read it and start again.
//              Two fake pads are plugged in: one in the standard layout, which the stop screen's
//              controller list must pick up through the Gamepad API, and one without it. Then the
//              main thread stalls for 400 ms, and the audio output (Bevy → cpal → Web Audio) must
//              not schedule a buffer in the past afterwards (vendor/README.md, cpal).
//
// Each fails on a panic, any console error (a shader the browser's WebGPU rejects, a pipeline or
// validation failure), a page error or a timeout. Saves target/web/smoke.png, but headless
// Chromium does not composite WebGPU canvases into screenshots, so the canvas shows blank there.
//
//   node tools/web-port/smoke.mjs [dist-dir]
//
// Needs Playwright with its Chromium (resolved through NODE_PATH, like a global install).
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { dirname, extname, join, normalize, resolve } from 'node:path';

const dist = resolve(process.argv[2] ?? 'target/web/dist');
const work = resolve(dist, '..');
const repository = resolve(dirname(new URL(import.meta.url).pathname), '../..');
const { chromium } = createRequire(import.meta.url)('playwright');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };

const ID = '0123456789abcdef0123456789abcdef';
const INPUT_CFG = 'assets/private/stock/data/config/input.cfg';

/** The native package layout: package/data/installation.json → data/installations/<id>. */
async function synthetic() {
  const pkg = join(work, 'synthetic-package');
  const inst = join(pkg, 'data/installations', ID);
  await rm(pkg, { recursive: true, force: true });
  const put = async (path, contents) => {
    await mkdir(dirname(join(pkg, path)), { recursive: true });
    await writeFile(join(pkg, path), contents);
  };
  await put('data/installation.json', JSON.stringify({ version: 1, directory: `installations/${ID}` }));
  await put('README.txt', 'Not part of the installation: the page must ignore it.\n');
  await put(`data/installations/${ID}/assets/private/game.json`, JSON.stringify({
    version: 1,
    character_scene: 'private/skater.glb',
    initial_animation: 'SYNTHETIC',
    action_graph: 'private/stock/data/state/ActionGraph_OnBoard.stategraph',
    motion_graph: 'private/stock/data/state/MotionGraph_OnBoard.stategraph',
  }));
  for (const placeholder of ['skater.glb', 'stock/data/state/ActionGraph_OnBoard.stategraph', 'stock/data/state/MotionGraph_OnBoard.stategraph']) {
    await put(`data/installations/${ID}/assets/private/${placeholder}`, '');
  }
  await put(`data/installations/${ID}/${INPUT_CFG}`, '// Synthetic placeholder: not a gameplay configuration.\n');
  await put(`data/installations/${ID}/settings/default-map.json`, JSON.stringify('maps/format-demo.skate'));
  await mkdir(join(inst, 'maps'), { recursive: true });
  await copyFile(join(repository, 'maps/format-demo.skate'), join(inst, 'maps/format-demo.skate'));
  const other = join(work, 'synthetic-not-ours');
  await rm(other, { recursive: true, force: true });
  await mkdir(join(other, 'assets'), { recursive: true });
  await writeFile(join(other, 'notes.txt'), 'No installation here.\n');
  return { pkg, other };
}

const server = createServer(async (request, response) => {
  const path = normalize(decodeURIComponent(new URL(request.url, 'http://x').pathname));
  try {
    const body = await readFile(join(dist, path.endsWith('/') ? `${path}index.html` : path));
    response.writeHead(200, { 'content-type': types[extname(path) || '.html'] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(0, 'localhost');
await new Promise((ready) => server.once('listening', ready));
const url = `http://localhost:${server.address().port}/`;
const folders = await synthetic();
const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--autoplay-policy=no-user-gesture-required'] });

/** Opens the page in a fresh profile; `choose` picks a folder through the directory input. */
async function run(name, { folder, withhold, gamepads, until, check, after }) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
  const page = await context.newPage();
  // Use the <input webkitdirectory> path: the File System Access picker cannot be automated.
  await page.addInitScript(() => {
    delete window.showDirectoryPicker;
    delete Window.prototype.showDirectoryPicker;
  });
  // Every Web Audio buffer start: when it was asked to play, against the audio clock.
  await page.addInitScript(() => {
    globalThis.skateAudioStarts = [];
    const start = AudioBufferSourceNode.prototype.start;
    AudioBufferSourceNode.prototype.start = function (when = 0, ...rest) {
      globalThis.skateAudioStarts.push({ when, now: this.context.currentTime, duration: this.buffer?.duration ?? 0 });
      return start.call(this, when, ...rest);
    };
  });
  if (gamepads) {
    // Gamepad objects as the browser makes them: the real prototypes, so the engine's type checks
    // pass, with own values in place of a device.
    await page.addInitScript(() => {
      const button = (pressed) => Object.create(GamepadButton.prototype, {
        pressed: { value: pressed }, touched: { value: pressed }, value: { value: pressed ? 1 : 0 },
      });
      const pad = (index, id, mapping) => Object.create(Gamepad.prototype, {
        id: { value: id }, index: { value: index }, connected: { value: true }, mapping: { value: mapping },
        timestamp: { value: 0 }, buttons: { value: Array.from({ length: 17 }, (_, i) => button(i === 0)) },
        axes: { value: [0.5, -0.5, 0, 0] },
      });
      const pads = [
        pad(0, 'Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)', 'standard'),
        pad(1, 'Unmapped Pad', ''),
      ];
      navigator.getGamepads = () => [...pads, null, null];
    });
  }
  if (withhold) {
    // Leave one file out of the first read-ahead, as a missing read-ahead rule would.
    await page.addInitScript((withheld) => {
      let installation;
      Object.defineProperty(globalThis, 'skateInstallation', {
        configurable: true,
        get: () => installation,
        set(value) {
          const prefetch = value.prefetch;
          let first = true;
          value.prefetch = (paths) => {
            if (first) paths = paths.filter((path) => path !== withheld);
            first = false;
            return prefetch(paths);
          };
          installation = value;
        },
      });
    }, withhold);
  }
  const lines = [];
  let failure = null;
  page.on('console', (message) => {
    const text = message.text();
    lines.push(text);
    if (text.includes('panicked at')) failure ??= `panic: ${text}`;
    else if (message.type() === 'error') failure ??= `console error: ${text}`;
  });
  page.on('pageerror', (error) => {
    // winit hands control back to the browser's event loop by throwing; that is not a failure.
    if (!String(error).includes('Using exceptions for control flow')) failure ??= `page error: ${error}`;
  });
  const started = Date.now();
  await page.goto(url);
  await page.getByText('Choose your installation folder to play.').waitFor({ timeout: 30_000 });
  if (folder) {
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Choose installation folder' }).click();
    await (await chooser).setFiles(folder);
  }
  const message = () => page.locator('#message').textContent();
  while (!failure && !(await until(lines, await message()))) {
    if (Date.now() - started > 240_000) failure = `timed out; the page says: ${await message()}`;
    await page.waitForTimeout(250);
  }
  if (!failure) failure = check(lines, await message());
  if (!failure && after) failure = await after(page);
  if (name === 'synthetic') await page.screenshot({ path: join(work, 'smoke.png') });
  await context.close();
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  if (failure) {
    console.error(`FAILED ${name} (${seconds} s): ${failure}`);
    console.error(lines.slice(-20).join('\n'));
    process.exitCode = 1;
  } else {
    console.log(`ok ${name} (${seconds} s): ${await Promise.resolve(lines.filter((l) => l.startsWith('SKATE_WEB')).length)} engine lines`);
  }
  return lines;
}

const seen = (lines, tag) => lines.find((line) => line.includes(tag));

await run('picker', {
  until: (_, message) => message.includes('Choose your installation folder'),
  check: (lines) => (seen(lines, 'SKATE_WEB') ? 'the engine started without a folder' : null),
});

await run('not-ours', {
  folder: folders.other,
  until: (_, message) => message.includes('No converted installation'),
  check: () => null,
});

const lines = await run('synthetic', {
  folder: folders.pkg,
  withhold: INPUT_CFG,
  gamepads: true,
  until: (lines) => seen(lines, 'SKATE_WEB_READY') && seen(lines, 'SKATE_WEB_STOPPED'),
  check(lines, message) {
    const stop = seen(lines, 'SKATE_WEB_STOPPED');
    if (!stop.includes('Input configuration') || !stop.includes('input.cfg')) return `stopped somewhere else: ${stop}`;
    if (!message.includes('Input configuration')) return `the page does not show the stop: ${message}`;
    if (!seen(lines, 'SKATE_WEB_READ_AHEAD attempt=1 files=1')) return 'the withheld file was not read on a retry';
    if (!seen(lines, 'Controller 0: gamepad "Xbox 360 Controller')) return 'the standard pad did not reach controller slot 0';
    if (!seen(lines, 'Gamepad "Unmapped Pad": the browser does not map it')) return 'the unmapped pad was not reported';
    if (seen(lines, 'Controller 1: gamepad')) return 'the unmapped pad took a slot';
    return null;
  },
  async after(page) {
    await page.waitForTimeout(1_000);
    const before = await page.evaluate(() => globalThis.skateAudioStarts.length);
    if (before === 0) return 'no audio output: Web Audio never started a buffer';
    // A main-thread stall longer than both of cpal's buffers, as a map load in the browser makes.
    await page.evaluate(() => { const end = performance.now() + 400; while (performance.now() < end); });
    await page.waitForTimeout(1_000);
    const starts = await page.evaluate(() => globalThis.skateAudioStarts);
    const later = starts.slice(before);
    if (later.length < 4) return `audio output stopped after the stall (${later.length} buffers)`;
    const late = later.filter((s) => s.when < s.now - 0.001);
    if (late.length) return `${late.length} of ${later.length} audio buffers after the stall were scheduled in the past (overlapped playback)`;
    console.log(`  audio: ${starts.length} buffers, ${later.length} after a 400 ms stall, none scheduled in the past`);
    return null;
  },
});
for (const tag of ['SKATE_WEB_READ_AHEAD', 'Controller 0:', 'SKATE_WEB_READY', 'SKATE_WEB_STOPPED']) {
  for (const line of lines.filter((l) => l.includes(tag))) console.log(`  ${line.slice(0, 200)}`);
}

await browser.close();
server.close();
