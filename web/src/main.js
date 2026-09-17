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
import { DetailSheet } from './ui/detail-sheet.js';
import { PlayerView } from './ui/player-view.js';
import { toast } from './ui/toast.js';

const host = new BridgeHost();
const coreLoader = new CoreLoader(host);
const audio = new AudioOutput(host);
const input = new InputManager(host);

const library = new LibraryView({
  scheduler: frameLoop,
  onOpenDetails: (entryId) => detail.open(entryId),
  onLaunch: (entryId) => launch(entryId),
});

const detail = new DetailSheet({
  scheduler: frameLoop,
  onLaunch: (entryId) => {
    detail.close();
    void launch(entryId);
  },
  onLoadState: (gameId, slot) => player.loadState(gameId, slot),
  onDataChanged: () => library.refreshData(),
});

const player = new PlayerView({
  host,
  coreLoader,
  audio,
  input,
  loop: frameLoop,
  onExit: () => {
    // "Continue playing" and the hero reflect what was just played.
    library.refreshData();
    frameLoop.wake(4);
  },
});

async function launch(entryId) {
  await player.launch(entryId);
  // A state saved during play should appear if the sheet is reopened.
  if (detail.isOpen) detail.reloadStates();
}

// ---------------------------------------------------------------------- wiring

library.mount();
input.attach();
audio.installGestureUnlock();

// UI work runs inside the same loop as emulation, after the engine tick.
frameLoop.addFlushTask(library.flush);
frameLoop.addFlushTask(detail.flush);
frameLoop.onError = (err) => host.reportError('Frame error', err);
frameLoop.wake(4);

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
    document.getElementById('status-cores').textContent =
      `Cores declared: ${coreLoader.manifest.size} · resident: ${host.bridge.residentCoreCount}`;
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
window.__continuum = { host, coreLoader, audio, input, library, detail, player, frameLoop };
