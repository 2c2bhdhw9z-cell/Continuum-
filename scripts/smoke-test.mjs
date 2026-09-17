#!/usr/bin/env node
/**
 * Browser smoke test for the Phase 1 shell.
 *
 * Checks the architectural invariants, not the styling — the things that are easy
 * to break silently and expensive to discover late:
 *
 *   1. No emulator core is loaded at boot (rule 5).
 *   2. DOM node count stays bounded while scrolling a 4,800-entry catalogue
 *      (rule 4) — measured before and after scrolling, in shelves and grid modes.
 *   3. There is exactly one `requestAnimationFrame` driver (rule 3), and it idles
 *      when nothing is happening.
 *   4. Rendering goes through WebGPU only (rule 2): no 2D context is ever obtained.
 *   5. The full launch path works: manifest → declare → fetch → attach → session,
 *      producing advancing frames and non-empty audio.
 *
 * Usage:
 *   node scripts/serve.mjs 8123 &
 *   node scripts/smoke-test.mjs [baseUrl]
 *
 * Requires `playwright-core` and a Chromium build. Neither is a project
 * dependency — there is no `package.json` here on purpose, since the app ships
 * without a bundler — so both are located via environment variables:
 *
 *   PLAYWRIGHT_CORE  path to a playwright-core installation (default: resolved
 *                    normally, e.g. after `npm i -g playwright-core`)
 *   CHROME_PATH      path to the Chromium binary
 */

import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const BASE_URL = process.argv[2] ?? 'http://localhost:8123/';
const CHROME =
  process.env.CHROME_PATH ?? '/opt/playwright/chromium-1232/chrome-linux64/chrome';

const { chromium } = require(process.env.PLAYWRIGHT_CORE ?? 'playwright-core');

const results = [];
let failures = 0;

function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  if (!ok) failures++;
  const mark = ok ? 'PASS' : 'FAIL';
  console.log(`${mark}  ${name}${detail ? ` — ${detail}` : ''}`);
}

/** Reports an environment observation that must not fail the run. */
function info(name, detail) {
  console.log(`INFO  ${name}${detail ? ` — ${detail}` : ''}`);
}

const browser = await chromium.launch({
  executablePath: CHROME,
  args: [
    // Headless WebGPU via SwiftShader. Without these there is no adapter and the
    // GPU half of the test cannot run at all.
    '--enable-unsafe-webgpu',
    '--enable-features=Vulkan',
    '--use-angle=swiftshader',
    '--use-gl=swiftshader',
    '--enable-webgpu-developer-features',
    // Audio: no gesture in an automated run, and no device to play to.
    '--autoplay-policy=no-user-gesture-required',
    '--mute-audio',
    '--no-sandbox',
  ],
});

const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await context.newPage();

/** @type {string[]} */
const consoleErrors = [];
page.on('console', (msg) => {
  if (msg.type() === 'error') consoleErrors.push(msg.text());
});
page.on('pageerror', (err) => consoleErrors.push(`pageerror: ${err.message}`));

// Failed requests deserve the URL, not just "404 (Not Found)" from the console.
/** @type {string[]} */
const failedRequests = [];
page.on('response', (response) => {
  if (response.status() >= 400) failedRequests.push(`${response.status()} ${response.url()}`);
});
page.on('requestfailed', (request) =>
  failedRequests.push(`failed ${request.url()} (${request.failure()?.errorText})`),
);

// Instrumentation installed before any app code runs: counts rAF registrations and
// traps any attempt to obtain a 2D canvas context.
await page.addInitScript(() => {
  window.__rafCalls = 0;
  const originalRaf = window.requestAnimationFrame;
  window.requestAnimationFrame = function (cb) {
    window.__rafCalls++;
    return originalRaf.call(window, cb);
  };

  // Watch the GPU device wgpu creates. Some environments (notably headless
  // Chromium on SwiftShader) destroy the device the moment a canvas swapchain is
  // touched, which turns every later GPU call into a silent no-op. Knowing that
  // happened is the difference between "our renderer is broken" and "this browser
  // cannot composite a WebGPU canvas".
  window.__deviceLost = null;
  if (typeof GPUAdapter !== 'undefined') {
    const originalRequestDevice = GPUAdapter.prototype.requestDevice;
    GPUAdapter.prototype.requestDevice = async function (...args) {
      const device = await originalRequestDevice.apply(this, args);
      device.lost.then((info) => {
        window.__deviceLost = `${info.reason}: ${info.message}`;
      });
      return device;
    };
  }

  window.__forbiddenContexts = [];
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, ...rest) {
    if (type === '2d' || type === 'bitmaprenderer') {
      window.__forbiddenContexts.push(type);
    }
    return originalGetContext.call(this, type, ...rest);
  };
});

console.log(`\n── loading ${BASE_URL}\n`);
await page.goto(BASE_URL, { waitUntil: 'load' });

// ---------------------------------------------------------------- 1. boot state

await page.waitForFunction(() => document.querySelectorAll('.card').length > 0, {
  timeout: 10_000,
});

const webgpu = await page.evaluate(() => 'gpu' in navigator);
check('browser exposes WebGPU', webgpu, webgpu ? '' : 'skipping GPU-dependent checks');

// The engine is warmed on idle; wait for the manifest to have been declared.
await page
  .waitForFunction(() => window.__continuum?.coreLoader?.manifest?.size > 0, {
    timeout: 15_000,
  })
  .catch(() => {});

const boot = await page.evaluate(() => ({
  declared: window.__continuum.coreLoader.manifest.size,
  resident: window.__continuum.host.bridge?.residentCoreCount ?? -1,
  status: window.__continuum.host.bridge?.status ?? 'not-loaded',
  cardCount: document.querySelectorAll('.card').length,
  catalog: document.getElementById('status-catalog').textContent,
}));

check('core manifest declared', boot.declared === 10, `${boot.declared} cores declared`);
check(
  'no cores loaded at boot (rule 5)',
  boot.resident === 0,
  `residentCoreCount = ${boot.resident}`,
);
check(
  'bridge is idle before launch',
  boot.status === 'uninitialised',
  `status = ${boot.status}`,
);
check(
  'catalogue indexed',
  /4,80\d titles/.test(boot.catalog),
  boot.catalog,
);

// The built-in test cart must be present and marked as real content, because it is
// what makes the real core demonstrable without shipping someone else's ROM.
const builtin = await page.evaluate(async () => {
  const { entryById } = await import('./src/data/catalog.js');
  const entry = entryById('builtin-nes-testcart');
  return entry ? { title: entry.title, real: entry.real, source: entry.source } : null;
});
check(
  'built-in NES test cart is registered as real content',
  builtin?.real === true && builtin.source === 'builtin',
  builtin ? `${builtin.title} (${builtin.source})` : 'missing',
);

// ------------------------------------------------- 2. virtualisation, shelves

const shelvesBefore = await page.evaluate(() => ({
  cards: document.querySelectorAll('.card').length,
  shelves: document.querySelectorAll('.shelf').length,
}));

// Scroll the whole shelf list, then a shelf horizontally through many pages.
await page.evaluate(async () => {
  const scroller = document.getElementById('library-scroll');
  for (let i = 0; i < 40; i++) {
    scroller.scrollTop += 400;
    await new Promise((r) => requestAnimationFrame(r));
  }
});
await page.waitForTimeout(200);

await page.evaluate(async () => {
  const track = document.querySelector('.shelf__track');
  if (!track) return;
  for (let i = 0; i < 60; i++) {
    track.scrollLeft += 600;
    await new Promise((r) => requestAnimationFrame(r));
  }
});
await page.waitForTimeout(200);

const shelvesAfter = await page.evaluate(() => ({
  cards: document.querySelectorAll('.card').length,
  shelves: document.querySelectorAll('.shelf').length,
  scrollTop: document.getElementById('library-scroll').scrollTop,
  bounded: window.__continuum.library.shelfScroller.assertBounded(),
  rowStats: window.__continuum.library.rowScrollers.map((s) => ({
    nodes: s.nodeCount,
    count: s.count,
  })),
}));

check(
  'shelf node count constant while scrolling (rule 4)',
  shelvesAfter.shelves === shelvesBefore.shelves && shelvesAfter.shelves <= 8,
  `${shelvesBefore.shelves} → ${shelvesAfter.shelves} shelf nodes`,
);
// The number that matters is *constancy*: scrolling 16,000 px through 4,800 titles
// must not add a single node. The absolute bound is a function of viewport size
// (7 shelves x ~15 cards at 1440x900), so it is checked per-scroller below rather
// than against a magic total.
check(
  'card node count constant while scrolling (rule 4)',
  shelvesAfter.cards === shelvesBefore.cards,
  `${shelvesBefore.cards} → ${shelvesAfter.cards} cards for 4,800 titles`,
);
check('vertical scroller pool within viewport bound', shelvesAfter.bounded === true);

const biggestShelf = shelvesAfter.rowStats.reduce(
  (max, s) => (s.count > max.count ? s : max),
  { nodes: 0, count: 0 },
);
check(
  'shelf holding hundreds of titles uses a small pool',
  biggestShelf.count > 100 && biggestShelf.nodes < 40,
  `${biggestShelf.count} titles → ${biggestShelf.nodes} nodes`,
);

// -------------------------------------------------------- 3. grid + search modes

const beforeGridMode = await page.evaluate(() => document.querySelectorAll('.card').length);
await page.click('.navlink[data-mode="grid"]');
await page.waitForTimeout(300);
const gridBefore = await page.evaluate(() => document.querySelectorAll('.card').length);
await page.evaluate(async () => {
  const scroller = document.getElementById('library-scroll');
  for (let i = 0; i < 80; i++) {
    scroller.scrollTop += 900;
    await new Promise((r) => requestAnimationFrame(r));
  }
});
await page.waitForTimeout(200);
const gridAfter = await page.evaluate(() => ({
  cards: document.querySelectorAll('.card').length,
  rows: document.querySelectorAll('.grid__row').length,
  rowCount: window.__continuum.library.gridScroller.count,
}));

check(
  'grid virtualises 4,800 titles',
  gridAfter.cards === gridBefore && gridAfter.rows < 12,
  `${gridAfter.rows} row nodes / ${gridAfter.rowCount} rows, ${gridAfter.cards} cards`,
);
check(
  'grid pool is built lazily, on first use',
  gridBefore > beforeGridMode,
  `${beforeGridMode} cards at boot → ${gridBefore} after entering grid mode`,
);

await page.fill('#search-input', 'dragon');
await page.waitForTimeout(400);
const searchResults = await page.evaluate(() => ({
  count: window.__continuum.library.gridIndices.length,
  cards: document.querySelectorAll('.card').length,
  heading: document.querySelector('.grid__title').textContent,
}));
// 151 results and 4,800 results must cost the same DOM: the node count is compared
// against the pre-search figure, not an arbitrary ceiling.
check(
  'search filters the index without growing the DOM',
  searchResults.count > 0 &&
    searchResults.count < 4800 &&
    searchResults.cards === gridAfter.cards,
  `${searchResults.count} matches, ${searchResults.cards} card nodes (unchanged)`,
);

await page.fill('#search-input', '');
await page.waitForTimeout(300);

// ------------------------------------------------ 4. detail sheet + state list

// Find a game with a long save-state history: that is the case worth testing.
const stateProbe = await page.evaluate(async () => {
  const { entryAt, catalogSize } = await import('./src/data/catalog.js');
  const states = await import('./src/data/save-states.js');
  for (let i = 0; i < catalogSize; i++) {
    const entry = entryAt(i);
    if (states.countFor(entry.id) > 120) {
      return { id: entry.id, count: states.countFor(entry.id) };
    }
  }
  return null;
});

if (stateProbe) {
  await page.evaluate((id) => window.__continuum.detail.open(id), stateProbe.id);
  await page.waitForTimeout(300);
  const sheet = await page.evaluate(async () => {
    const el = document.getElementById('detail-states');
    for (let i = 0; i < 30; i++) {
      el.scrollTop += 200;
      await new Promise((r) => requestAnimationFrame(r));
    }
    return {
      rows: document.querySelectorAll('.state-row').length,
      total: window.__continuum.detail.states.length,
      open: !document.getElementById('detail-sheet').hidden,
    };
  });
  check(
    'save-state list is virtualised',
    sheet.open && sheet.rows < 15 && sheet.total > 120,
    `${sheet.total} states → ${sheet.rows} row nodes`,
  );
  await page.evaluate(() => window.__continuum.detail.close());
} else {
  check('save-state list is virtualised', false, 'no game with >120 states found');
}

// --------------------------------------------------------------- 5. launch path

if (webgpu) {
  // A synthetic entry on a placeholder-core system: exercises the GPU path without
  // needing real content. NES is deliberately avoided here — it now runs a real core,
  // which rightly refuses a catalogue placeholder.
  const target = await page.evaluate(async () => {
    const { entryAt, catalogSize } = await import('./src/data/catalog.js');
    const { getSystem } = await import('./src/data/systems.js');
    for (let i = 0; i < catalogSize; i++) {
      const entry = entryAt(i);
      const system = getSystem(entry.systemId);
      if (system?.phase === 1 && entry.systemId !== 'nes' && !entry.real) {
        return { id: entry.id, title: entry.title, systemId: entry.systemId };
      }
    }
    return null;
  });

  // ---- Pixel pipeline, verified offscreen ------------------------------------
  //
  // Runs before anything touches the canvas swapchain, driving the bridge directly:
  // init GPU → attach core → launch → capture. No present happens, so this measures
  // upload → shader → scaling → readback in isolation, and it is the check that
  // would catch a flipped blit, a channel-order mistake, or broken letterbox maths.
  const pixels = await page.evaluate(async (entryId) => {
    const { entryById } = await import('./src/data/catalog.js');
    const { getContent } = await import('./src/data/content-store.js');
    const host = window.__continuum.host;
    const loader = window.__continuum.coreLoader;
    const entry = entryById(entryId);

    // The canvas must be laid out before GPU init: a hidden canvas measures zero.
    document.getElementById('app').dataset.view = 'player';
    document.getElementById('view-player').hidden = false;

    await host.load();
    await host.initGpu(document.getElementById('gpu-canvas'));
    const coreId = loader.coreIdFor(entry.systemId);
    await loader.ensureCore(coreId, () => {});
    const { contentFilename } = await import('./src/data/content-store.js');
    host.bridge.launch(coreId, entry.id, await getContent(entry), contentFilename(entry));

    const summarise = (rgba, w, h) => {
      const at = (x, y) => {
        const o = (y * w + x) * 4;
        return [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]];
      };
      const distinct = new Set();
      for (let i = 0; i < rgba.length; i += 4 * 101) {
        distinct.add(`${rgba[i]},${rgba[i + 1]},${rgba[i + 2]}`);
      }
      return {
        length: rgba.length,
        distinct: distinct.size,
        corner: at(6, 6),
        centre: at(w >> 1, h >> 1),
        edge: at(2, h >> 1),
      };
    };

    // 4:3 target matches the core's aspect: the image fills it edge to edge.
    const fitted = summarise(await host.captureFrame(320, 240), 320, 240);
    // 2:1 target must letterbox, leaving bars at the left and right edges.
    const wide = summarise(await host.captureFrame(400, 200), 400, 200);

    host.bridge.stop();
    document.getElementById('view-player').hidden = true;
    document.getElementById('app').dataset.view = 'library';
    return { fitted, wide, deviceLost: window.__deviceLost };
  }, target.id);

  check(
    'GPU device survives init and offscreen rendering',
    pixels.deviceLost === null,
    pixels.deviceLost ?? 'alive',
  );
  check(
    'GPU readback returns a full frame',
    pixels.fitted.length === 320 * 240 * 4,
    `${pixels.fitted.length} bytes`,
  );
  check(
    'rendered frame carries the core image, not a flat clear',
    pixels.fitted.distinct > 8,
    `${pixels.fitted.distinct} distinct sampled colours`,
  );
  check(
    "core's top-left marker lands top-left (blit orientation)",
    pixels.fitted.corner[0] > 180 &&
      pixels.fitted.corner[1] < 130 &&
      pixels.fitted.corner[3] === 255,
    `corner rgba = ${pixels.fitted.corner.join(',')}`,
  );
  check(
    'aspect-fit letterboxes a mismatched target',
    pixels.wide.edge[0] < 20 &&
      pixels.wide.edge[1] < 20 &&
      pixels.wide.centre[3] === 255 &&
      pixels.wide.distinct > 4,
    `edge ${pixels.wide.edge.slice(0, 3).join(',')} vs centre ${pixels.wide.centre
      .slice(0, 3)
      .join(',')}`,
  );

  // ---- Full session through the player and the shared loop -------------------
  await page.evaluate((id) => window.__continuum.player.launch(id), target.id);

  // Wait for the session to actually be emulating, not merely requested.
  const launched = await page
    .waitForFunction(
      () => window.__continuum.host.bridge?.status === 'running',
      { timeout: 25_000 },
    )
    .then(() => true)
    .catch(() => false);

  check('session reaches "running"', launched, launched ? target.title : 'timed out');

  if (launched) {
    const afterLaunch = await page.evaluate(() => ({
      resident: window.__continuum.host.bridge.residentCoreCount,
      adapter: window.__continuum.host.bridge.adapterInfo,
      coreState: window.__continuum.host.bridge.coreState(
        window.__continuum.coreLoader.coreIdFor(
          window.__continuum.player.entry.systemId,
        ),
      ),
    }));
    check(
      'core loaded only on launch (rule 5)',
      afterLaunch.resident === 1 && afterLaunch.coreState === 'bound',
      `resident = ${afterLaunch.resident}, state = ${afterLaunch.coreState}`,
    );
    check(
      'WebGPU adapter acquired through wgpu',
      typeof afterLaunch.adapter === 'string' && afterLaunch.adapter.length > 0,
      afterLaunch.adapter ?? 'none',
    );

    // Let it run for ~1.5 s and confirm frames and audio advance.
    const first = await page.evaluate(() => ({ ...window.__continuum.host.stats }));
    await page.waitForTimeout(1500);
    const second = await page.evaluate(() => ({
      stats: { ...window.__continuum.host.stats },
      presented: true,
      audioSubmitted: window.__continuum.host.stats.audioQueuedFrames,
      workletStats: { ...window.__continuum.audio.workletStats },
      audioState: window.__continuum.audio.state,
      hudFps: document.getElementById('hud-fps').textContent,
      hudFrames: document.getElementById('hud-frames').textContent,
    }));

    const advanced = second.stats.frameCount - first.frameCount;
    check(
      'core frames advance under the rAF loop',
      advanced > 30,
      `${advanced} frames in ~1.5 s (HUD reads ${second.hudFps} fps, ${second.hudFrames} frames)`,
    );
    check(
      'frames are presented to the swapchain',
      second.stats.presented === true,
      `presented = ${second.stats.presented}`,
    );

    // Canvas compositing: reported, not asserted. Headless SwiftShader destroys the
    // WebGPU device as soon as a canvas swapchain is touched, so this cannot be a
    // pass/fail criterion here. The offscreen pixel checks above are what verify the
    // render path.
    const composited = await page.evaluate(async () => {
      const canvas = document.getElementById('gpu-canvas');
      const bitmap = await createImageBitmap(canvas);
      const off = new OffscreenCanvas(bitmap.width, bitmap.height);
      const ctx = off.getContext('2d');
      ctx.drawImage(bitmap, 0, 0);
      const p = ctx.getImageData(bitmap.width >> 1, bitmap.height >> 1, 1, 1).data;
      return { size: [bitmap.width, bitmap.height], centre: [...p], lost: window.__deviceLost };
    });
    info(
      'canvas compositing (environment, not asserted)',
      composited.centre[3] !== 0
        ? `canvas centre rgba = ${composited.centre.join(',')}`
        : `canvas reads back empty at ${composited.size.join('x')}; device state: ` +
          `${composited.lost ?? 'alive'} — a known headless/SwiftShader limitation`,
    );

    // Cumulative counters, not queue depth: the pump drains the ring every tick,
    // so an instantaneous reading is legitimately ~0 even when audio is flowing.
    const submitted = second.stats.audioFramesSubmitted - first.audioFramesSubmitted;
    const drained = second.stats.audioFramesDrained - first.audioFramesDrained;
    check(
      'audio PCM flows core → sink → host',
      submitted > 20_000 && drained > 20_000,
      `${submitted} frames submitted, ${drained} drained in ~1.5 s ` +
        `(ctx ${second.audioState}, worklet queue ${second.workletStats.queuedFrames})`,
    );

    // Pause: emulation stops, presentation continues. Stats refresh on the next
    // tick, so settle before sampling the baseline.
    await page.evaluate(() => window.__continuum.player.togglePause());
    await page.waitForTimeout(150);
    const pausedFrames = await page.evaluate(
      () => window.__continuum.host.stats.frameCount,
    );
    await page.waitForTimeout(400);
    const stillPaused = await page.evaluate(() => ({
      frames: window.__continuum.host.stats.frameCount,
      presented: window.__continuum.host.stats.presented,
    }));
    check(
      'pause halts emulation but keeps presenting',
      stillPaused.frames === pausedFrames && stillPaused.presented === true,
      `frame count held at ${stillPaused.frames}, presented = ${stillPaused.presented}`,
    );

    await page.evaluate(() => window.__continuum.player.togglePause());
    await page.waitForTimeout(300);
    const resumed = await page.evaluate(() => window.__continuum.host.stats.frameCount);
    check(
      'resume continues emulation',
      resumed > stillPaused.frames,
      `${stillPaused.frames} → ${resumed} frames`,
    );

    // Save-state round trip through the real core: the frame counter is part of
    // the serialised state, so a correct load rewinds it.
    const stateRoundTrip = await page.evaluate(async () => {
      const player = window.__continuum.player;
      const host = window.__continuum.host;
      const record = player.saveState();
      if (!record) return { ok: false, reason: 'save returned nothing' };
      await new Promise((r) => setTimeout(r, 60));
      const savedAt = host.stats.frameCount;
      await new Promise((r) => setTimeout(r, 300));
      const advanced = host.stats.frameCount;
      player.loadState(player.entry.id, record.slot);
      await new Promise((r) => setTimeout(r, 100));
      return { ok: true, slot: record.slot, savedAt, advanced, restored: host.stats.frameCount };
    });
    check(
      'save + load state rewinds the core',
      stateRoundTrip.ok &&
        stateRoundTrip.advanced > stateRoundTrip.savedAt &&
        Math.abs(stateRoundTrip.restored - stateRoundTrip.savedAt) <= 6,
      stateRoundTrip.ok
        ? `slot ${stateRoundTrip.slot}: saved at ${stateRoundTrip.savedAt}, ran to ` +
          `${stateRoundTrip.advanced}, restored to ${stateRoundTrip.restored}`
        : stateRoundTrip.reason,
    );

    await page.evaluate(() => window.__continuum.player.exit());
    await page.waitForTimeout(300);
    const afterExit = await page.evaluate(() => ({
      status: window.__continuum.host.bridge.status,
      resident: window.__continuum.host.bridge.residentCoreCount,
      engineActive: window.__continuum.frameLoop.engineActive,
    }));
    check(
      'exit stops the engine and keeps the core warm',
      afterExit.status === 'idle' && afterExit.resident === 1 && !afterExit.engineActive,
      `status = ${afterExit.status}, resident = ${afterExit.resident}`,
    );
  }
}

// ------------------------------------------- 5b. the real libretro core (fceumm)
//
// The headline of Phase 1b: a genuine NES core, compiled from C to wasm, running the
// project's own test ROM through the same bridge, pacer, sink and renderer as
// everything else. `scripts/core-abi-test.mjs` verifies the emulation itself
// (pixels, input, scrolling, audio) headlessly; what matters here is that the
// browser path — fetch, instantiate, attach, launch, tick — works end to end.
if (webgpu) {
  await page.evaluate(() => window.__continuum.player.launch('builtin-nes-testcart'));
  const running = await page
    .waitForFunction(() => window.__continuum.host.bridge?.status === 'running', {
      timeout: 40_000,
    })
    .then(() => true)
    .catch(() => false);

  check('real NES core reaches "running" with the test cart', running);

  if (running) {
    const info = await page.evaluate(() => ({
      avInfo: Array.from(window.__continuum.host.bridge.sessionAvInfo() ?? []),
      coreName: window.__continuum.host.bridge.sessionCoreName,
      coreId: window.__continuum.coreLoader.coreIdFor('nes'),
      kind: window.__continuum.coreLoader.entryFor('fceumm')?.kind,
    }));

    check(
      'core module is a real libretro build, not a placeholder',
      info.kind === 'libretro' && info.coreId === 'fceumm' && info.coreName === 'FCEUmm',
      `${info.coreName} via '${info.coreId}' (kind: ${info.kind})`,
    );

    // These values come from the core's own retro_get_system_av_info, so matching them
    // proves the descriptor was refreshed from the core rather than trusted from the
    // manifest — 48000 Hz in particular is a value the manifest could not know.
    const [width, height, aspect, fps, sampleRate] = info.avInfo;
    check(
      'core reports NES geometry and timing to the bridge',
      width === 256 &&
        height === 240 &&
        Math.abs(fps - 60.0998) < 0.01 &&
        sampleRate === 48000,
      `${width}x${height}, aspect ${aspect.toFixed(4)}, ${fps.toFixed(4)} fps, ${sampleRate} Hz`,
    );

    // Let the new session's first ticks land before sampling. `host.stats` still holds
    // the *previous* session's counters until a tick of this one overwrites them, and
    // every counter (frame count, audio frames) restarts at zero for a new core — so
    // sampling too early produces a negative delta.
    await page.waitForTimeout(250);
    const before = await page.evaluate(() => ({ ...window.__continuum.host.stats }));
    await page.waitForTimeout(1500);
    const after = await page.evaluate(() => ({ ...window.__continuum.host.stats }));

    const frames = after.frameCount - before.frameCount;
    check(
      'real emulation advances at NES speed',
      frames > 45 && frames < 110,
      `${frames} frames in ~1.5 s`,
    );

    // fceumm emits 48 kHz; the test ROM drives pulse channel 1, so this is real APU
    // output crossing core → JS → Rust sink → host.
    const audioFrames = after.audioFramesSubmitted - before.audioFramesSubmitted;
    // Counted after resampling, so the rate matches the *device* (typically 44.1 kHz),
    // not the core's 48 kHz — which is itself evidence the resampler ran.
    check(
      'real APU audio flows through the Rust sink',
      audioFrames > 50_000 && audioFrames < 90_000,
      `${audioFrames} frames in ~1.5 s (~${Math.round(audioFrames / 1.5)} Hz at the device rate, ` +
        'resampled from the core\'s 48 kHz)',
    );

    // Input through the full stack: JS → GamepadBridge → snapshot → CoreHost →
    // core's input_state callback. The ROM turns the screen red while A is held; the
    // pixel proof of that is in the headless ABI test, so here we assert the plumbing
    // survives a press without disturbing emulation.
    const withInput = await page.evaluate(async () => {
      const bridge = window.__continuum.host.bridge;
      bridge.setButton(0, 8, true, 'keyboard'); // A
      const start = window.__continuum.host.stats.frameCount;
      await new Promise((r) => setTimeout(r, 400));
      const held = window.__continuum.host.stats.frameCount;
      bridge.setButton(0, 8, false, 'keyboard');
      // A virtual pad, to prove the gamepad path is wired too.
      bridge.connectPad(1, 'gamepad', 'Smoke Test Pad');
      bridge.applyGamepad(1, new Uint8Array([0, 1, 0, 0]), new Float32Array([0, 0, 0, 0]));
      return { advanced: held - start, pads: bridge.connectedPads };
    });
    check(
      'input reaches the running core without stalling it',
      withInput.advanced > 10 && withInput.pads >= 1,
      `${withInput.advanced} frames while A held, ${withInput.pads} pad(s) registered`,
    );

    await page.evaluate(() => window.__continuum.player.exit());
    await page.waitForTimeout(300);
    const afterExit = await page.evaluate(() => ({
      status: window.__continuum.host.bridge.status,
      resident: window.__continuum.host.bridge.residentCoreCount,
    }));
    check(
      'real core survives session teardown',
      afterExit.status === 'idle' && afterExit.resident >= 1,
      `status ${afterExit.status}, ${afterExit.resident} core(s) resident`,
    );
  }
}

// ---------------------------------------------------- 6. rendering + loop rules

const forbidden = await page.evaluate(() => window.__forbiddenContexts);
check(
  'no 2D canvas context is ever created (rule 2)',
  forbidden.length === 0,
  forbidden.length ? forbidden.join(', ') : 'only WebGPU used',
);

// The loop must idle when nothing is happening, rather than spinning forever.
const idleProbe = await page.evaluate(async () => {
  await new Promise((r) => setTimeout(r, 600));
  const before = window.__rafCalls;
  await new Promise((r) => setTimeout(r, 600));
  return { before, after: window.__rafCalls, running: window.__continuum.frameLoop.running };
});
check(
  'frame loop idles when there is no work (rule 3)',
  idleProbe.after === idleProbe.before && !idleProbe.running,
  `rAF registrations stable at ${idleProbe.after}`,
);

check(
  'no failed network requests',
  failedRequests.length === 0,
  failedRequests.slice(0, 4).join(' | ') || 'clean',
);

check(
  'no console errors',
  consoleErrors.length === 0,
  consoleErrors.slice(0, 3).join(' | ') || 'clean',
);

// ------------------------------------------------------------------- reporting

await page.screenshot({ path: '/tmp/continuum-library.png', fullPage: false });
await browser.close();

console.log(
  `\n── ${results.length - failures}/${results.length} checks passed` +
    (failures ? `, ${failures} FAILED` : '') +
    '\n',
);
process.exit(failures === 0 ? 0 : 1);
