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
    // Exposes `globalThis.gc()`, which the core-swap leak check needs: whether a core
    // module was *freed* can only be observed after a real collection, and waiting for
    // one to happen by chance would make the test flaky.
    '--js-flags=--expose-gc',
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
  libretroCores: [...window.__continuum.coreLoader.manifest.values()]
    .filter((core) => core.kind === 'libretro')
    .map((core) => core.id),
  // Asked of the registry rather than of a JS-side map: the system -> cores mapping
  // lives in Rust, and that is the thing worth asserting on.
  systemsWithRealCores: [
    ...new Set(
      [...window.__continuum.coreLoader.manifest.values()]
        .filter((core) => core.kind === 'libretro')
        .flatMap((core) => core.systems ?? []),
    ),
  ].filter(
    (system) =>
      window.__continuum.coreLoader.entryFor(
        window.__continuum.coreLoader.coreIdFor(system),
      )?.kind === 'libretro',
  ),
  // Systems more than one core declares. This is the subcore relationship, read
  // straight out of the registry.
  multiCoreSystems: Object.fromEntries(
    [
      ...new Set(
        [...window.__continuum.coreLoader.manifest.values()].flatMap(
          (core) => core.systems ?? [],
        ),
      ),
    ]
      .map((system) => [
        system,
        window.__continuum.host.bridge.coresForSystem(system),
      ])
      .filter(([, ids]) => ids.length > 1),
  ),
  resident: window.__continuum.host.bridge?.residentCoreCount ?? -1,
  status: window.__continuum.host.bridge?.status ?? 'not-loaded',
  cardCount: document.querySelectorAll('.card').length,
  catalog: document.getElementById('status-catalog').textContent,
}));

// Counted, not hard-coded: the point is that real cores are declared alongside the
// placeholders and that they claim the systems they can actually run.
check(
  'manifest declares all four real cores plus placeholders',
  boot.declared >= 8 &&
    ['fceumm', 'mgba', 'genesis_plus_gx', 'snes9x'].every((id) =>
      boot.libretroCores.includes(id),
    ),
  `${boot.declared} declared; real: ${boot.libretroCores.join(', ')}; ` +
    `systems with real cores: ${boot.systemsWithRealCores.join(', ')}`,
);
// The chain a dropped file actually travels: bytes → system → core. Worth asserting
// end to end, because each link lives somewhere different (header sniffing in
// rom-detect.js, the system→core mapping in the Rust registry) and nothing else proves
// they agree.
const routing = await page.evaluate(async () => {
  const { detectSystem } = await import('./src/data/rom-detect.js');
  const loader = window.__continuum.coreLoader;
  const probe = async (url, filename) => {
    const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
    const detected = detectSystem(bytes, filename);
    return {
      filename,
      systemId: detected.systemId,
      confidence: detected.confidence,
      coreId: detected.systemId ? loader.coreIdFor(detected.systemId) : null,
    };
  };
  return {
    carts: [
      await probe('./roms/nes-testcart.nes', 'nes-testcart.nes'),
      await probe('./roms/gba-testcart.gba', 'gba-testcart.gba'),
      await probe('./roms/sms-testcart.sms', 'sms-testcart.sms'),
      await probe('./roms/snes-testcart.sfc', 'snes-testcart.sfc'),
    ],
    // Headerless files under the other SNES extensions a real dump might carry.
    aliases: ['dump.smc', 'dump.swc', 'dump.fig'].map((name) => {
      const detected = detectSystem(new Uint8Array(64), name);
      return {
        name,
        systemId: detected.systemId,
        coreId: detected.systemId ? loader.coreIdFor(detected.systemId) : null,
      };
    }),
  };
});

const expectedRouting = { nes: 'fceumm', gba: 'mgba', sms: 'genesis_plus_gx', snes: 'snes9x' };
check(
  'each test cart routes from its bytes to the right real core',
  routing.carts.every((c) => c.coreId === expectedRouting[c.systemId]),
  routing.carts
    .map((c) => `${c.filename} → ${c.systemId} (${c.confidence}) → ${c.coreId}`)
    .join('; '),
);
check(
  '.smc / .swc / .fig all resolve to the SNES core by extension',
  routing.aliases.every((a) => a.systemId === 'snes' && a.coreId === 'snes9x'),
  routing.aliases.map((a) => `${a.name} → ${a.systemId}/${a.coreId}`).join(', '),
);

check(
  'a system with two cores resolves to the higher priority one, keeping the other available',
  boot.multiCoreSystems.gb?.length === 2 &&
    boot.multiCoreSystems.gb[0] === 'mgba' &&
    boot.multiCoreSystems.gb.includes('gambatte'),
  Object.entries(boot.multiCoreSystems)
    .map(([system, ids]) => `${system}: ${ids.join(' > ')}`)
    .join('; ') || 'no system has more than one core',
);
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

// Synthetic histories are a UI fixture. Inventing them for a ROM the user actually
// owns would be the UI lying about their data — and every "Load" button would fail.
const realStates = await page.evaluate(async () => {
  const states = await import('./src/data/save-states.js');
  return states.countFor('builtin-nes-testcart');
});
check(
  'real content starts with no invented save states',
  realStates === 0,
  `${realStates} states for the built-in cart`,
);

// --------------------------------------------------------------- 5. launch path

if (webgpu) {
  // A synthetic entry whose system is served by a *placeholder* core: that exercises
  // the GPU path without needing real content, because the diagnostic core draws its
  // pattern from nothing while a real core rightly refuses catalogue noise.
  //
  // The predicate asks the registry which core would run the entry rather than naming
  // systems to avoid. An earlier version excluded 'nes' by hand and then broke the day
  // SNES got a real core — the property that matters is "is this core a placeholder",
  // so that is what gets tested.
  const target = await page.evaluate(async () => {
    const { entryAt, catalogSize } = await import('./src/data/catalog.js');
    const { getSystem } = await import('./src/data/systems.js');
    const loader = window.__continuum.coreLoader;
    for (let i = 0; i < catalogSize; i++) {
      const entry = entryAt(i);
      const system = getSystem(entry.systemId);
      if (system?.phase !== 1 || entry.real) continue;
      const coreId = loader.coreIdFor(entry.systemId);
      if (!coreId || loader.entryFor(coreId)?.kind === 'libretro') continue;
      return { id: entry.id, title: entry.title, systemId: entry.systemId, coreId };
    }
    return null;
  });
  if (!target) {
    // Cannot happen while any phase-1 system is still served by a placeholder. If it
    // ever does, these checks need to be repointed at a real cart — they verify blit
    // orientation, letterbox maths and the whole upload path, so quietly skipping
    // them would be much worse than stopping here.
    throw new Error(
      'no synthetic entry is left on a placeholder core: every phase-1 system now has ' +
        'a real core, so the offscreen GPU checks need a real cart as their subject',
    );
  }

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
      // Saving and loading are asynchronous now that states are persisted, so both
      // have to be awaited — a floating promise here would compare a frame counter
      // against a state that had not been written yet.
      const record = await player.saveState();
      if (!record) return { ok: false, reason: 'save returned nothing' };
      await new Promise((r) => setTimeout(r, 60));
      const savedAt = host.stats.frameCount;
      await new Promise((r) => setTimeout(r, 300));
      const advanced = host.stats.frameCount;
      await player.loadState(player.entry.id, record.slot);
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
    // Under the default retention policy, exiting frees the core rather than keeping it
    // warm: on a multi-system emulator, an idle core is tens of megabytes doing nothing.
    check(
      'exit stops the engine and frees the core',
      afterExit.status === 'idle' && afterExit.resident === 0 && !afterExit.engineActive,
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
      'real core is released on teardown',
      afterExit.status === 'idle' && afterExit.resident === 0,
      `status ${afterExit.status}, ${afterExit.resident} core(s) resident`,
    );
  }
}

// ------------------------------------------- 5c. multi-core hot swapping
//
// The Phase 2 question: can we switch systems repeatedly without leaking a core?
//
// Each core is a wasm module with its own linear memory — 2 MB of code plus 16–32 MB of
// working memory for mGBA. Leaking one per swap exhausts a phone in a handful of
// switches, and "we called destroy()" is not evidence. So this swaps NES↔GBA several
// times and then asks three independent questions:
//
//   1. Did every runtime get torn down?          instantiated === destroyed
//   2. Did the host actually reclaim them?       FinalizationRegistry, after a real GC
//   3. Did the *engine's* memory stay flat?      Rust wasm memory before vs after
//
// It also checks the picture: the two test ROMs idle in different colours, so the core
// that produced a frame is identifiable from pixels rather than from bookkeeping.
if (webgpu) {
  const SWAPS = 3;
  /** One built-in cart per real core, so a round exercises all four. */
  const SWAP_TARGETS = [
    ['builtin-nes-testcart', 'FCEUmm', 'nes'],
    ['builtin-gba-testcart', 'mGBA', 'gba'],
    ['builtin-sms-testcart', 'Genesis Plus GX', 'sms'],
    ['builtin-snes-testcart', 'Snes9x', 'snes'],
  ];
  const SESSIONS = SWAPS * SWAP_TARGETS.length;
  const swapLog = [];
  let swapFailure = null;
  let peakCoreMb = 0;

  const beforeSwaps = await page.evaluate(() => ({
    engineMemory: window.__continuum.host.memory.buffer.byteLength,
    instantiated: window.__continuum.runtimeStats.instantiated,
  }));

  for (let round = 0; round < SWAPS; round++) {
    for (const [entryId, expectedCore, expectedSystem] of SWAP_TARGETS) {
      const result = await page.evaluate(
        async ([id, core]) => {
          const { player, host, runtimeStats } = window.__continuum;
          await player.launch(id);

          // Wait for the session to be genuinely running before measuring.
          const deadline = Date.now() + 30_000;
          while (host.bridge.status !== 'running' && Date.now() < deadline) {
            await new Promise((r) => setTimeout(r, 50));
          }
          if (host.bridge.status !== 'running') return { ok: false, reason: `${id} never ran` };

          await new Promise((r) => setTimeout(r, 400));
          const framesAtStart = host.stats.frameCount;
          await new Promise((r) => setTimeout(r, 350));

          const running = {
            core: host.bridge.sessionCoreName,
            avInfo: Array.from(host.bridge.sessionAvInfo() ?? []),
            resident: host.bridge.residentCoreCount,
            residentIds: host.bridge.residentCoreIds(),
            advanced: host.stats.frameCount - framesAtStart,
            audioQueued: host.stats.audioQueuedFrames,
            liveRuntimes: runtimeStats.live.length,
            // The core's own reported footprint while running, which includes the heap
            // it grew after loading content.
            coreMb: +((host.bridge.sessionCoreMemoryBytes ?? 0) / 1024 / 1024).toFixed(1),
          };

          player.exit();
          await new Promise((r) => setTimeout(r, 300));

          const idle = {
            coreMemoryAfterExit: host.bridge.sessionCoreMemoryBytes ?? 0,
            status: host.bridge.status,
            resident: host.bridge.residentCoreCount,
            liveRuntimes: runtimeStats.live.length,
            liveBytes: runtimeStats.liveBytes,
            engineActive: window.__continuum.frameLoop.engineActive,
            audioQueued: host.stats.audioQueuedFrames,
            workletQueued: window.__continuum.audio.workletStats.queuedFrames,
          };

          return { ok: true, expectedCore: core, running, idle };
        },
        [entryId, expectedCore],
      );

      if (!result.ok) {
        swapFailure ??= result.reason;
        continue;
      }

      const { running, idle } = result;
      // While running: the right core, alone in memory, and actually emulating.
      if (running.core !== expectedCore) {
        swapFailure ??= `expected ${expectedCore}, got ${running.core}`;
      }
      if (running.resident !== 1) {
        swapFailure ??= `${running.resident} cores resident during ${expectedCore} (expected 1): ${running.residentIds}`;
      }
      if (running.advanced < 10) {
        swapFailure ??= `${expectedCore} advanced only ${running.advanced} frames`;
      }
      // After exit: nothing resident, nothing running, nothing queued.
      if (idle.resident !== 0) {
        swapFailure ??= `${idle.resident} cores still resident after exiting ${expectedCore}`;
      }
      if (idle.liveRuntimes !== 0) {
        swapFailure ??= `${idle.liveRuntimes} runtime(s) not destroyed after exiting ${expectedCore}`;
      }
      if (idle.engineActive) {
        swapFailure ??= `frame loop still active after exiting ${expectedCore}`;
      }
      if (idle.status !== 'idle') {
        swapFailure ??= `status '${idle.status}' after exiting ${expectedCore}`;
      }

      if (idle.coreMemoryAfterExit !== 0) {
        swapFailure ??= `core memory still reported after exiting ${expectedCore}`;
      }
      peakCoreMb = Math.max(peakCoreMb, running.coreMb);

      swapLog.push(
        `${expectedSystem}:${running.core} ${running.avInfo[0]}x${running.avInfo[1]}` +
          `@${running.avInfo[3].toFixed(2)} ${running.coreMb}MB ${running.advanced}f`,
      );
    }
  }

  check(
    `cycled NES → GBA → Master System → SNES ${SWAPS}x with one core resident at a time`,
    swapFailure === null,
    swapFailure ?? swapLog.join(' | '),
  );

  // Switching straight from one game to another, with no trip back to the library. This
  // is the path that would hold two cores at once if teardown were ordered wrongly:
  // fetch-and-instantiate B, *then* free A.
  const directSwitch = await page.evaluate(async () => {
    const { player, host, runtimeStats, audio } = window.__continuum;
    const waitForRunning = async () => {
      const deadline = Date.now() + 30_000;
      while (host.bridge.status !== 'running' && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 50));
      }
      return host.bridge.status === 'running';
    };

    await player.launch('builtin-nes-testcart');
    if (!(await waitForRunning())) return { ok: false, reason: 'NES never ran' };
    const first = { core: host.bridge.sessionCoreName, live: runtimeStats.live.length };

    // Straight into the GBA cart without exiting.
    await player.launch('builtin-gba-testcart');
    if (!(await waitForRunning())) return { ok: false, reason: 'GBA never ran' };
    await new Promise((r) => setTimeout(r, 400));
    const second = {
      core: host.bridge.sessionCoreName,
      live: runtimeStats.live.length,
      resident: host.bridge.residentCoreCount,
      avInfo: Array.from(host.bridge.sessionAvInfo() ?? []),
    };

    player.exit();
    await new Promise((r) => setTimeout(r, 500));
    return {
      ok: true,
      first,
      second,
      maxLive: runtimeStats.maxLive,
      afterExit: {
        live: runtimeStats.live.length,
        resident: host.bridge.residentCoreCount,
        // Producer side, and the authoritative one: read live from the Rust sink, which
        // is replaced on stop. A non-zero queue here would mean stale audio could still
        // be delivered into the next session.
        sinkQueued: host.bridge.audioQueuedFrames,
        // The cached mirror must be cleared too, or the HUD keeps showing the finished
        // session's numbers.
        mirrorFrameCount: host.stats.frameCount,
        audioState: audio.state,
        // Reported for context only. The worklet's stats freeze when the context
        // suspends (no `process()` calls), so this figure is last-known, not current.
        workletQueued: audio.workletStats.queuedFrames,
        engineActive: window.__continuum.frameLoop.engineActive,
      },
    };
  });

  check(
    'switching game-to-game replaces the core without stacking',
    directSwitch.ok &&
      directSwitch.first.core === 'FCEUmm' &&
      directSwitch.second.core === 'mGBA' &&
      directSwitch.second.resident === 1 &&
      directSwitch.second.avInfo[0] === 240,
    directSwitch.ok
      ? `${directSwitch.first.core} → ${directSwitch.second.core} ` +
        `(${directSwitch.second.avInfo[0]}x${directSwitch.second.avInfo[1]}), ` +
        `${directSwitch.second.resident} resident`
      : directSwitch.reason,
  );

  // The high-water mark is the real proof: a momentary overlap during a switch would be
  // invisible to any check made after the fact.
  check(
    'never more than one core alive at any instant',
    directSwitch.maxLive === 1,
    `high-water mark: ${directSwitch.maxLive} simultaneous core runtime(s)`,
  );

  check(
    'teardown drains audio, stops the loop and frees the core',
    directSwitch.ok &&
      directSwitch.afterExit.live === 0 &&
      directSwitch.afterExit.resident === 0 &&
      directSwitch.afterExit.sinkQueued === 0 &&
      directSwitch.afterExit.mirrorFrameCount === 0 &&
      directSwitch.afterExit.audioState === 'suspended' &&
      !directSwitch.afterExit.engineActive,
    directSwitch.ok
      ? `${directSwitch.afterExit.live} live cores, sink queue ` +
        `${directSwitch.afterExit.sinkQueued}, audio ${directSwitch.afterExit.audioState}, ` +
        `engine ${directSwitch.afterExit.engineActive ? 'active' : 'stopped'} ` +
        `(worklet last reported ${directSwitch.afterExit.workletQueued} frames before suspending)`
      : directSwitch.reason,
  );

  // Force a collection and let the FinalizationRegistry callbacks run. Without
  // `--expose-gc` this is skipped rather than guessed at.
  const leaks = await page.evaluate(async () => {
    const stats = window.__continuum.runtimeStats;
    const canGc = typeof globalThis.gc === 'function';
    if (canGc) {
      // Twice, with a turn of the event loop between: the first pass drops the objects,
      // the second lets finalizers be scheduled.
      globalThis.gc();
      await new Promise((r) => setTimeout(r, 100));
      globalThis.gc();
      await new Promise((r) => setTimeout(r, 200));
    }
    return {
      canGc,
      instantiated: stats.instantiated,
      destroyed: stats.destroyed,
      collected: stats.collected,
      live: stats.live.length,
      liveBytes: stats.liveBytes,
      peakLiveMb: +(stats.peakLiveBytes / 1024 / 1024).toFixed(1),
      engineMemory: window.__continuum.host.memory.buffer.byteLength,
    };
  });

  check(
    'every core runtime was torn down',
    leaks.instantiated === leaks.destroyed && leaks.live === 0 && leaks.liveBytes === 0,
    `${leaks.instantiated} instantiated, ${leaks.destroyed} destroyed, ${leaks.live} live, ` +
      `peak ${leaks.peakLiveMb} MB held at once`,
  );

  // The strong claim: the host reclaimed the modules. Only the collector can attest to
  // this, which is why it is measured separately from our own counters.
  if (leaks.canGc) {
    check(
      'core wasm modules were garbage collected (no retained references)',
      leaks.collected >= leaks.instantiated - 1,
      `${leaks.collected}/${leaks.instantiated} collected after forced GC`,
    );
  } else {
    info('core module collection', 'skipped: --expose-gc not available');
  }

  // Peak memory is the multi-core policy's whole point: one core at a time. The bound is
  // generous (a GBA core with a loaded ROM is tens of MB); what matters is that it does
  // not scale with the number of swaps.
  check(
    'never held more than one core in memory',
    peakCoreMb > 0 && peakCoreMb < 96,
    `largest running core measured at ${peakCoreMb} MB over ${SESSIONS} sessions`,
  );

  // The engine's own memory cannot shrink (wasm memory never does), so the test is that
  // it stops growing: staging buffers are per-session and must be released, not stacked.
  const growthMb = (leaks.engineMemory - beforeSwaps.engineMemory) / 1024 / 1024;
  check(
    'engine memory does not grow across swaps',
    growthMb < 8,
    `${(beforeSwaps.engineMemory / 1024 / 1024).toFixed(1)} MB → ` +
      `${(leaks.engineMemory / 1024 / 1024).toFixed(1)} MB over ${SESSIONS} sessions ` +
      `(+${growthMb.toFixed(1)} MB)`,
  );
}

// ------------------------------------------------------- 5e. subcore selection
//
// A system maps to many cores, and the user can override which one runs. Three things
// have to hold, and only the third needs a browser:
//
//   1. Resolution: a preference selects an alternative, and a preference that cannot
//      apply is ignored rather than obeyed. (Also unit-tested in Rust; asserted here
//      to prove the facade is wired to the same logic.)
//   2. UI: the picker appears only where there is a choice to make.
//   3. Launch: the override actually changes which core runs — proved from the
//      framebuffer, because the built-in Master System cart draws a magenta playfield
//      on the real core and the placeholder draws the diagnostic pattern instead.

const resolution = await page.evaluate(() => {
  const { host, coreLoader, corePrefs } = window.__continuum;
  const bridge = host.bridge;
  const before = corePrefs.get('sms');
  corePrefs.set('sms', null);

  const out = {
    smsCandidates: bridge.coresForSystem('sms'),
    gbCandidates: bridge.coresForSystem('gb'),
    nesCandidates: bridge.coresForSystem('nes'),
    defaultSms: coreLoader.coreIdFor('sms'),
    overriddenSms: coreLoader.coreIdFor('sms', 'smsplus'),
    // A core that exists but cannot run this system.
    wrongCore: coreLoader.coreIdFor('sms', 'fceumm'),
    // A core that is not declared at all.
    unknownCore: coreLoader.coreIdFor('sms', 'no-such-core'),
    // Stored preference, read back through the same path the launcher uses.
    storedTakesEffect: null,
    storedCleared: null,
  };

  corePrefs.set('sms', 'smsplus');
  out.storedTakesEffect = coreLoader.coreIdFor('sms', corePrefs.get('sms'));
  corePrefs.set('sms', null);
  out.storedCleared = coreLoader.coreIdFor('sms', corePrefs.get('sms'));

  corePrefs.set('sms', before);
  return out;
});

check(
  'a system lists every core that can run it, best first',
  resolution.smsCandidates.length === 2 &&
    resolution.smsCandidates[0] === 'genesis_plus_gx' &&
    resolution.gbCandidates.length === 2 &&
    resolution.nesCandidates.length === 1,
  `sms: ${resolution.smsCandidates.join(' > ')} · gb: ${resolution.gbCandidates.join(' > ')} · ` +
    `nes: ${resolution.nesCandidates.join(' > ')}`,
);
check(
  'a preference selects an alternative core',
  resolution.defaultSms === 'genesis_plus_gx' &&
    resolution.overriddenSms === 'smsplus' &&
    resolution.storedTakesEffect === 'smsplus' &&
    resolution.storedCleared === 'genesis_plus_gx',
  `default ${resolution.defaultSms}, explicit ${resolution.overriddenSms}, ` +
    `stored ${resolution.storedTakesEffect}, cleared ${resolution.storedCleared}`,
);
check(
  'a preference that cannot apply is ignored, not obeyed',
  resolution.wrongCore === 'genesis_plus_gx' && resolution.unknownCore === 'genesis_plus_gx',
  `core for another system → ${resolution.wrongCore}; undeclared core → ${resolution.unknownCore}`,
);

// The picker is a choice, so it must not appear where there is nothing to choose.
const picker = await page.evaluate(() => {
  const { detail } = window.__continuum;
  const read = (entryId) => {
    detail.open(entryId);
    const row = document.getElementById('detail-core-row');
    const select = document.getElementById('detail-core');
    const result = {
      shown: !row.hidden,
      options: [...select.options].map((o) => o.textContent),
      value: select.value,
    };
    detail.close();
    return result;
  };
  return { sms: read('builtin-sms-testcart'), nes: read('builtin-nes-testcart') };
});

check(
  'detail sheet offers a core picker only when the system has more than one core',
  picker.sms.shown &&
    picker.sms.options.length === 3 &&
    picker.sms.value === '' &&
    !picker.nes.shown,
  `Master System: ${picker.sms.options.join(' / ')} · NES picker shown: ${picker.nes.shown}`,
);

// The gesture itself, not just the menu component: right-click is delegated from the
// scroll container because cards are recycled, and that wiring is easy to break.
const gesture = await page.evaluate(() => {
  const { coreMenu, detail } = window.__continuum;
  const fire = (entryId) => {
    coreMenu.close();
    const card = [...document.querySelectorAll('.card')].find(
      (c) => c.dataset.entryId === entryId,
    );
    if (!card) return { error: `no card for ${entryId}` };
    const rect = card.getBoundingClientRect();
    const event = new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: Math.round(rect.left + 20),
      clientY: Math.round(rect.top + 20),
    });
    card.dispatchEvent(event);
    return {
      open: coreMenu.isOpen,
      // A menu that opened must claim the event; one that did not must let the
      // browser show its own.
      defaultPrevented: event.defaultPrevented,
      items: [...document.querySelectorAll('.coremenu__item')].map((i) => i.dataset.coreId),
    };
  };
  const multi = fire('builtin-sms-testcart');
  const single = fire('builtin-nes-testcart');
  coreMenu.close();
  detail.close();
  return { multi, single };
});

check(
  'right-clicking a card opens the subcore menu, and only where there is a choice',
  gesture.multi.open &&
    gesture.multi.defaultPrevented &&
    gesture.multi.items.join() === 'genesis_plus_gx,smsplus' &&
    !gesture.single.open &&
    !gesture.single.defaultPrevented,
  `Master System card → ${gesture.multi.items.join(', ')} (browser menu suppressed: ` +
    `${gesture.multi.defaultPrevented}) · NES card → menu ${gesture.single.open ? 'opened' : 'not opened'}`,
);

if (webgpu) {
  // The end-to-end check. Same ROM, same content path, two different cores — and the
  // difference is read off the framebuffer rather than from what the UI claims.
  const override = await page.evaluate(async () => {
    const { player, host, runtimeStats, corePrefs } = window.__continuum;
    const before = corePrefs.get('sms');

    const runWith = async (coreId) => {
      await player.launch('builtin-sms-testcart', coreId ? { coreId } : {});
      const deadline = Date.now() + 30_000;
      while (host.bridge.status !== 'running' && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 50));
      }
      if (host.bridge.status !== 'running') return { ok: false, reason: 'never ran' };
      await new Promise((r) => setTimeout(r, 500));

      // Best-effort. By this point in the run the earlier compositing probe has
      // usually destroyed the WebGPU device (SwiftShader tears it down on
      // getCurrentTexture), and a GPU readback cannot be mapped afterwards. The core
      // identity below does not depend on the GPU, so a failed capture downgrades the
      // pixel evidence rather than the check.
      let rgb = null;
      let captureError = null;
      try {
        const rgba = await host.captureFrame(256, 192);
        let r = 0;
        let g = 0;
        let b = 0;
        let n = 0;
        // Inset to skip any letterbox the fit introduces at the edges.
        for (let y = 24; y < 168; y += 3) {
          for (let x = 32; x < 224; x += 3) {
            const o = (y * 256 + x) * 4;
            r += rgba[o];
            g += rgba[o + 1];
            b += rgba[o + 2];
            n++;
          }
        }
        rgb = [Math.round(r / n), Math.round(g / n), Math.round(b / n)];
      } catch (err) {
        captureError = err.message;
      }

      const out = {
        ok: true,
        core: host.bridge.sessionCoreName,
        resident: host.bridge.residentCoreIds(),
        // Geometry comes from the core that is actually loaded, so it is independent
        // evidence of which one answered.
        avInfo: Array.from(host.bridge.sessionAvInfo() ?? []),
        rgb,
        captureError,
      };
      player.exit();
      await new Promise((r2) => setTimeout(r2, 300));
      return out;
    };

    const real = await runWith(null);
    const alternative = await runWith('smsplus');

    corePrefs.set('sms', before);
    return {
      real,
      alternative,
      instantiated: runtimeStats.instantiated,
      destroyed: runtimeStats.destroyed,
      maxLive: runtimeStats.maxLive,
      live: runtimeStats.live.length,
    };
  });

  const { real, alternative } = override;
  // Magenta: red and blue clearly above green. This is the cart's own palette, so it
  // can only appear if Genesis Plus GX actually emulated it.
  const isMagenta = (rgb) => rgb[0] > rgb[1] + 20 && rgb[2] > rgb[1] + 20;

  check(
    'default core runs the Master System cart',
    real.ok && real.core === 'Genesis Plus GX' && real.resident.join() === 'genesis_plus_gx',
    real.ok ? `${real.core} (resident: ${real.resident.join(', ')})` : real.reason,
  );
  check(
    'overriding the core changes which one actually runs',
    alternative.ok &&
      alternative.core === 'SMS Plus GX' &&
      alternative.resident.join() === 'smsplus',
    alternative.ok
      ? `same ROM, same launch path: ${real.core} → ${alternative.core} ` +
          `(resident: ${alternative.resident.join(', ')})`
      : alternative.reason,
  );

  // Pixel confirmation when the environment allows a readback. The Node ABI harness
  // asserts the cart's colours unconditionally, so this is corroboration rather than
  // the only evidence.
  if (real.rgb && alternative.rgb) {
    check(
      'the two cores draw different pictures from the same ROM',
      isMagenta(real.rgb) && !isMagenta(alternative.rgb),
      `real core rgb ${real.rgb.join(',')} (magenta) vs placeholder rgb ` +
        `${alternative.rgb.join(',')}`,
    );
  } else {
    info(
      'pixel comparison of the two cores (environment, not asserted)',
      `GPU readback unavailable: ${real.captureError ?? alternative.captureError} — ` +
        'the cart colours are asserted in scripts/core-abi-test.mjs instead',
    );
  }
  check(
    'switching cores for one system still holds only one core at a time',
    override.live === 0 && override.instantiated === override.destroyed && override.maxLive === 1,
    `${override.instantiated} instantiated, ${override.destroyed} destroyed, ` +
      `${override.live} live, high-water mark ${override.maxLive}`,
  );
}

// ------------------------------------------------- 5f. save-state persistence
//
// The claim is that progress survives closing the app. Testing that properly means
// proving the bytes reached IndexedDB, not that a Map in memory has an entry — so the
// index is thrown away and rehydrated from disk through the same code path boot uses,
// and only then are the records inspected.
//
// A page reload would be the most faithful test of all, but it would also reset the
// runtime counters the leak checks above depend on. Discarding and rehydrating the
// index exercises everything except `window.load`.

if (webgpu) {
  const persistence = await page.evaluate(async () => {
    const states = await import('./src/data/save-states.js');
    const { player, host } = window.__continuum;
    const GAME = 'builtin-nes-testcart';

    const waitForRunning = async () => {
      const deadline = Date.now() + 30_000;
      while (host.bridge.status !== 'running' && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 50));
      }
      return host.bridge.status === 'running';
    };

    // --- write a manual state and an auto state -----------------------------
    await player.launch(GAME);
    if (!(await waitForRunning())) return { ok: false, reason: 'first launch never ran' };
    await new Promise((r) => setTimeout(r, 700));

    const manual = await player.saveState();
    const frameAtSave = Math.round(host.stats.frameCount);
    // Kept for the byte-exactness comparison further down. This is the payload as it
    // existed in memory; everything after the rehydrate step has been through disk.
    const savedPayload = Array.from(host.bridge.saveState());

    // Let emulation move well past the save point, so a resume is distinguishable
    // from simply having started the game again.
    await new Promise((r) => setTimeout(r, 700));
    const frameAtExit = Math.round(host.stats.frameCount);

    player.exit();
    // exit() captures synchronously but writes asynchronously; give the transaction
    // a moment to commit, which is the same race a real app close has.
    await new Promise((r) => setTimeout(r, 600));

    // --- forget everything, then read it back off disk ----------------------
    states.__resetIndexForTests();
    const hydrateResult = await states.hydrate();
    const rehydrated = states.listFor(GAME);
    const auto = states.autoStateFor(GAME);
    const manualAfter = states.manualStatesFor(GAME).filter((s) => !s.synthetic);

    // --- payload really is on disk -----------------------------------------
    const payload = auto ? await states.payloadFor(GAME, states.AUTO_SLOT) : null;

    // --- compatibility gate -------------------------------------------------
    const sameCore = auto
      ? states.compatibility(auto, {
          coreId: auto.coreId,
          coreVersion: auto.coreVersion,
          stateSize: auto.stateSize,
        })
      : null;
    const wrongCore = auto
      ? states.compatibility(auto, { coreId: 'mgba', coreVersion: '0.11' })
      : null;
    const wrongVersion = auto
      ? states.compatibility(auto, { coreId: auto.coreId, coreVersion: 'not-the-same' })
      : null;
    const wrongSize = auto
      ? states.compatibility(auto, {
          coreId: auto.coreId,
          coreVersion: auto.coreVersion,
          stateSize: (auto.stateSize ?? 0) + 1,
        })
      : null;

    // --- resume on launch ---------------------------------------------------
    await player.launch(GAME);
    if (!(await waitForRunning())) return { ok: false, reason: 'relaunch never ran' };
    await new Promise((r) => setTimeout(r, 400));
    const subtitle = document.getElementById('player-subtitle').textContent;
    const frameAfterResume = Math.round(host.stats.frameCount);
    await new Promise((r) => setTimeout(r, 300));
    const frameStillAdvancing = Math.round(host.stats.frameCount);

    // --- restoring a disk payload is byte-exact -----------------------------
    //
    // The strongest available proof that persistence works: pause first, so no frames
    // run between the load and the capture, then load the manual slot — whose bytes
    // have been through IndexedDB and back — and re-serialise. A correct restore
    // reproduces the payload exactly.
    //
    // This is what the resume path cannot prove on its own. The NES test cart is so
    // simple that a fresh boot and an 84-frame-old checkpoint differ by only ~46 of
    // 13758 bytes, so no byte comparison can distinguish "resumed" from "started
    // again" — but it can prove that stored bytes land in the core untouched.
    player.togglePause();
    await new Promise((r) => setTimeout(r, 120));
    await player.loadState(GAME, 0);
    await new Promise((r) => setTimeout(r, 60));
    const afterLoad = Array.from(host.bridge.saveState());
    let differingBytes = 0;
    for (let i = 0; i < Math.max(afterLoad.length, savedPayload.length); i++) {
      if (afterLoad[i] !== savedPayload[i]) differingBytes++;
    }
    player.togglePause();

    player.exit();
    await new Promise((r) => setTimeout(r, 600));

    return {
      ok: true,
      manualSlot: manual?.slot ?? null,
      frameAtSave,
      frameAtExit,
      hydrateResult,
      rehydratedCount: rehydrated.length,
      manualCount: manualAfter.length,
      auto: auto
        ? {
            slot: auto.slot,
            frame: auto.frame,
            coreId: auto.coreId,
            coreName: auto.coreName,
            coreVersion: auto.coreVersion,
            stateSize: auto.stateSize,
          }
        : null,
      payloadLength: payload?.length ?? 0,
      gate: {
        same: sameCore?.ok,
        wrongCore: wrongCore?.ok,
        wrongVersion: wrongVersion?.ok,
        wrongSize: wrongSize?.ok,
        wrongCoreReason: wrongCore?.reason,
        wrongVersionReason: wrongVersion?.reason,
        wrongSizeReason: wrongSize?.reason,
      },
      subtitle,
      frameAfterResume,
      frameStillAdvancing,
      differingBytes,
      payloadBytes: savedPayload.length,
    };
  });

  if (!persistence.ok) {
    check('save states persist to IndexedDB', false, persistence.reason);
  } else {
    const p = persistence;
    check(
      'a manual save survives the index being discarded and rehydrated from disk',
      p.manualSlot !== null && p.manualCount >= 1 && p.hydrateResult.states >= 2,
      `slot ${p.manualSlot} written; after rehydrate: ${p.hydrateResult.states} state(s) ` +
        `across ${p.hydrateResult.games} game(s), ${p.manualCount} manual`,
    );
    check(
      'exiting auto-saves without anyone pressing Save',
      p.auto !== null && p.auto.frame >= p.frameAtSave,
      p.auto
        ? `auto state at frame ${p.auto.frame} (saved manually at ${p.frameAtSave}, ` +
            `exited at ${p.frameAtExit})`
        : 'no auto state was written',
    );
    check(
      'the payload is really on disk, not just its metadata',
      p.payloadLength > 0 && p.payloadLength === p.auto?.stateSize,
      `${p.payloadLength} bytes read back, metadata says ${p.auto?.stateSize}`,
    );
    check(
      'states are tagged with the core that wrote them',
      Boolean(p.auto?.coreId && p.auto?.coreVersion),
      p.auto ? `${p.auto.coreId} / ${p.auto.coreName} ${p.auto.coreVersion}` : 'untagged',
    );
    check(
      'the compatibility gate accepts its own core and refuses everything else',
      p.gate.same === true &&
        p.gate.wrongCore === false &&
        p.gate.wrongVersion === false &&
        p.gate.wrongSize === false,
      `same core ok=${p.gate.same}; different core → ${p.gate.wrongCoreReason}; ` +
        `different version → ${p.gate.wrongVersionReason}; ` +
        `different size → ${p.gate.wrongSizeReason}`,
    );
    check(
      'relaunching restores the checkpoint and keeps running',
      p.subtitle.includes('resumed') && p.frameStillAdvancing > p.frameAfterResume,
      `subtitle "${p.subtitle}"; emulation advanced ${p.frameAfterResume} → ` +
        `${p.frameStillAdvancing} frames after resuming`,
    );
    check(
      'a state that has been through IndexedDB restores byte-for-byte',
      p.differingBytes === 0 && p.payloadBytes > 0,
      `${p.differingBytes} of ${p.payloadBytes} bytes differ after loading the stored ` +
        'payload into a paused core',
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
