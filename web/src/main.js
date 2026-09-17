/**
 * Application entry point: constructs the pieces and wires them together.
 *
 * Boot order is chosen so the library is interactive as early as possible:
 *
 *   1. Build the library UI from the in-memory catalogue. No network, no wasm — the
 *      shelves are on screen before anything else is requested.
 *   2. Register the single frame loop's UI flush tasks.
 *   3. On idle, warm the wasm bridge and read the core *manifest*. Metadata only:
 *      not one byte of emulator code is fetched here.
 *   4. Cores load when a game is launched, and never before.
 *
 * The bridge is warmed on idle rather than lazily on first launch purely for
 * responsiveness: by the time anyone has picked a game, the engine is ready, but a
 * user who only browses never pays for GPU or core initialisation.
 */

import { frameLoop } from './engine/loop.js';
import { BridgeHost } from './engine/bridge-host.js';
import { CoreLoader } from './engine/core-loader.js';
import { InputManager } from './engine/input.js';
import { AudioOutput } from './audio/audio-output.js';
import { LibraryView } from './ui/library-view.js';
import { RomImporter } from './ui/rom-import.js';
import { registerBuiltins } from './data/builtins.js';
import { runtimeStats } from './engine/core-runtime.js';
import { DetailSheet } from './ui/detail-sheet.js';
import { PlayerView } from './ui/player-view.js';
import { CoreMenu } from './ui/core-menu.js';
import { entryById } from './data/catalog.js';
import { getCorePreference, setCorePreference } from './data/core-prefs.js';
import { toast } from './ui/toast.js';

const host = new BridgeHost();
const coreLoader = new CoreLoader(host);
const audio = new AudioOutput(host);
const input = new InputManager(host);

const library = new LibraryView({
  scheduler: frameLoop,
  onOpenDetails: (entryId) => detail.open(entryId),
  onLaunch: (entryId) => launch(entryId),
  onCoreMenu: (entryId, x, y) => {
    const entry = entryById(entryId);
    if (!entry) return false;
    return coreMenu.open({
      entryId,
      systemId: entry.systemId,
      title: entry.title,
      x,
      y,
    });
  },
});

/**
 * The subcore picker. Choosing a core records the preference for that system and
 * launches immediately — one stored choice per system, whether it was made here or in
 * the detail sheet, so there is only ever one answer to "which core will this use".
 */
const coreMenu = new CoreMenu({
  coresForSystem: (systemId) => coreLoader.coresForSystem(systemId),
  currentCoreId: (systemId) => coreLoader.coreIdFor(systemId, getCorePreference(systemId)),
  onChoose: (entryId, coreId) => {
    const entry = entryById(entryId);
    if (entry) setCorePreference(entry.systemId, coreId);
    void launch(entryId, { coreId });
  },
});

const detail = new DetailSheet({
  scheduler: frameLoop,
  onLaunch: (entryId) => {
    detail.close();
    void launch(entryId);
  },
  onLoadState: (gameId, slot) => player.loadState(gameId, slot),
  onDataChanged: () => library.refreshData(),
  onRemoveRom: (entryId) => importer.remove(entryId),
  coresForSystem: (systemId) => coreLoader.coresForSystem(systemId),
});

const importer = new RomImporter({
  onLibraryChanged: () => library.refreshData(),
  onPlay: (entryId) => void launch(entryId),
});

/**
 * Reports core residency in the status bar.
 *
 * Worth showing rather than hiding: this is the number that proves the multi-core
 * memory policy is working. Browsing the library should read "0 resident · 0.0 MB",
 * and swapping systems should never show two cores at once.
 */
function updateCoreStatus() {
  const el = document.getElementById('status-cores');
  if (!el) return;
  const bridge = host.bridge;
  const resident = bridge?.residentCoreCount ?? 0;
  const ids = bridge ? bridge.residentCoreIds() : [];
  // While a session runs, report the core's *live* footprint (it grows as the core
  // allocates); otherwise report what the lifecycle tracker still considers live, which
  // should be zero when browsing.
  const sessionBytes = bridge?.sessionCoreMemoryBytes ?? 0;
  const liveMb = ((sessionBytes || runtimeStats.liveBytes) / 1024 / 1024).toFixed(1);
  el.textContent =
    `Cores: ${coreLoader.manifest.size} declared · ${resident} resident · ${liveMb} MB live`;
  el.title = ids.length
    ? `Resident: ${ids.join(', ')} (retention: ${bridge?.coreRetention})`
    : 'No core is loaded while browsing the library';
}

const player = new PlayerView({
  host,
  coreLoader,
  audio,
  input,
  loop: frameLoop,
  onRequestImport: () => importer.openPicker(),
  onExit: () => {
    // "Continue playing" and the hero reflect what was just played.
    library.refreshData();
    updateCoreStatus();
    frameLoop.wake(4);
  },
});

async function launch(entryId, options) {
  await player.launch(entryId, options);
  updateCoreStatus();
  // A state saved during play should appear if the sheet is reopened.
  if (detail.isOpen) detail.reloadStates();
}

// ---------------------------------------------------------------------- wiring

// Content that actually exists is registered before the UI mounts, so the hero and
// the first shelf show something playable rather than a placeholder.
registerBuiltins();

library.mount();
input.attach();
audio.installGestureUnlock();

// UI work runs inside the same loop as emulation, after the engine tick.
frameLoop.addFlushTask(library.flush);
frameLoop.addFlushTask(detail.flush);
frameLoop.onError = (err) => host.reportError('Frame error', err);
frameLoop.wake(4);

// Previously imported ROMs are restored asynchronously; the library refreshes when
// they arrive rather than blocking first paint on IndexedDB.
void importer.restoreLibrary();

// ------------------------------------------------------------------- GPU badge

const gpuDot = document.querySelector('#gpu-pill .pill__dot');
const gpuLabel = document.getElementById('gpu-pill-label');

function setGpuBadge(state, label, title) {
  gpuDot.dataset.state = state;
  gpuLabel.textContent = label;
  document.getElementById('gpu-pill').title = title ?? label;
}

if (!BridgeHost.webgpuAvailable) {
  setGpuBadge(
    'bad',
    'No WebGPU',
    'This build renders exclusively through WebGPU. Chrome/Edge 113+, Safari 18+ or Firefox 141+ required.',
  );
  toast(
    'WebGPU unavailable',
    'Games cannot start in this browser. Rendering is WebGPU-only by design — there is no canvas 2D fallback.',
    { kind: 'error', ms: 9000 },
  );
} else {
  setGpuBadge('pending', 'WebGPU', 'WebGPU is available; the adapter is requested on first launch.');
}

document.getElementById('gpu-pill').addEventListener('click', () => {
  const info = host.bridge?.adapterInfo;
  toast(
    'Graphics',
    info
      ? `Adapter: ${info}`
      : BridgeHost.webgpuAvailable
        ? 'WebGPU is available. The adapter and device are created when a game launches.'
        : 'WebGPU is not available in this browser.',
    { ms: 6000 },
  );
});

// --------------------------------------------------------------- deferred boot

/** Warms the engine and declares cores. Never loads a core binary. */
async function warmEngine() {
  try {
    await host.load();
    await coreLoader.loadManifest();
    updateCoreStatus();
    if (BridgeHost.webgpuAvailable) {
      setGpuBadge('ok', 'WebGPU ready', 'Engine loaded. Adapter is created on first launch.');
    }
  } catch (err) {
    host.reportError('Engine failed to load', err);
    setGpuBadge('bad', 'Engine error', String(err?.message ?? err));
  }
}

if ('requestIdleCallback' in window) {
  requestIdleCallback(() => void warmEngine(), { timeout: 1500 });
} else {
  setTimeout(() => void warmEngine(), 300);
}

// ---------------------------------------------------------------------- PWA

if ('serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('./sw.js').catch((err) => {
      // Offline support is a bonus; the app works fine without it.
      console.warn('[pwa] service worker registration failed', err);
    });
  });
}

// Exposed for console-driven debugging and for the smoke test in
// `scripts/smoke-test.mjs`, which drives these directly.
window.__continuum = {
  host,
  runtimeStats,
  coreLoader,
  audio,
  input,
  library,
  detail,
  player,
  importer,
  coreMenu,
  frameLoop,
  launch,
  corePrefs: { get: getCorePreference, set: setCorePreference },
};
