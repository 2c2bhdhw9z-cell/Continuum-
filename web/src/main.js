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
import * as saveStates from './data/save-states.js';
import * as cheatStore from './data/cheats.js';
import { requestPersistentStorage, storageEstimate } from './data/idb.js';
import { runtimeStats } from './engine/core-runtime.js';
import { DetailSheet } from './ui/detail-sheet.js';
import { SettingsSheet } from './ui/settings-sheet.js';
import { CheatSheet } from './ui/cheat-sheet.js';
import { CoreOptionsSheet } from './ui/core-options-sheet.js';
import { TouchEditor } from './ui/touch-editor.js';
import { PlayerView } from './ui/player-view.js';
import { CoreMenu } from './ui/core-menu.js';
import { allIndices, entryAt, entryById } from './data/catalog.js';
import { getCorePreference, setCorePreference } from './data/core-prefs.js';
import { toast } from './ui/toast.js';
import { applyTheme } from './data/settings.js';

const host = new BridgeHost();
const coreLoader = new CoreLoader(host);
const audio = new AudioOutput(host);
const input = new InputManager(host);

const library = new LibraryView({
  scheduler: frameLoop,
  onOpenDetails: (entryId) => detail.open(entryId),
  onLaunch: (entryId) => launch(entryId),
  onOpenSettings: () => void settings.open(),
  onRequestImport: () => importer.openPicker(),
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
  onLoadState: (gameId, slot) => void player.loadState(gameId, slot),
  onDataChanged: () => library.refreshData(),
  onRemoveRom: (entryId) => importer.remove(entryId),
  onOpenCheats: (entryId) => {
    detail.close();
    cheatSheet.open(entryId);
  },
  onClearResume: async (entryId) => {
    await saveStates.clearAuto(entryId);
    detail.reloadStates();
    toast('Resume point cleared', 'This game will start from the beginning.');
  },
  coresForSystem: (systemId) => coreLoader.coresForSystem(systemId),
});

/**
 * The cheat manager. Opened from the detail sheet before a launch and from the player's
 * controls during one; both edit the same stored list.
 */
const cheatSheet = new CheatSheet({
  host,
  runningGameId: () => host.bridge?.currentContentId ?? null,
  onChanged: () => library.refreshData(),
});

/** The core's own options, read out of the running core rather than from a table here. */
const coreOptionsSheet = new CoreOptionsSheet({
  host,
  runningCoreId: () => host.bridge?.currentCoreId ?? null,
  runningCoreName: () =>
    coreLoader.entryFor(host.bridge?.currentCoreId ?? '')?.name ??
    host.bridge?.currentCoreId ??
    'the core',
});

/** Drag-to-arrange for the on-screen pad. */
const touchEditor = new TouchEditor();

const importer = new RomImporter({
  onLibraryChanged: () => library.refreshData(),
  onPlay: (entryId) => void launch(entryId),
});

/**
 * Settings. Reached from the fourth nav tab, which opens it as a sheet rather than
 * switching the library into a fourth mode.
 */
const settings = new SettingsSheet({
  coresForSystem: (systemId) => coreLoader.coresForSystem(systemId),
  coresReady: coreLoader.manifestReady,
  onSettingChanged: () => {
    player.syncHudVisibility();
    player.syncDisplaySettings();
  },
  onLibraryCleared: () => {
    // The store is empty, so the in-memory library is too. Re-registering the built-in
    // carts immediately is what keeps "clear everything" from leaving a dead app: the
    // four test carts are part of the build, not part of the user's data.
    registerBuiltins();
    library.refreshData();
    if (detail.isOpen) detail.close();
  },
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
  // A captured thumbnail has to reach the shelves: the card the user is about to
  // return to is the one that was blank when they launched it.
  onOpenCheats: (entryId) => cheatSheet.open(entryId),
  onOpenCoreOptions: () => coreOptionsSheet.open(),
  onEditLayout: () => touchEditor.start(),
  onArtworkCaptured: () => {
    library.refreshData();
    if (detail.isOpen) detail.reloadStates();
  },
  onStatesChanged: () => {
    // The detail sheet is usually closed during play, but if it is open behind the
    // player its list is now out of date.
    if (detail.isOpen) detail.reloadStates();
  },
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

// Applied before the library mounts, so the first paint is already in the chosen theme
// rather than flashing the default one.
applyTheme();

library.mount();
input.attach();
// The saved pad layout is read asynchronously; the stylesheet's own defaults are in force
// until it arrives, so the pad is usable either way.
void touchEditor.init();
audio.installGestureUnlock();

// UI work runs inside the same loop as emulation, after the engine tick.
frameLoop.addFlushTask(library.flush);
frameLoop.addFlushTask(detail.flush);
frameLoop.onError = (err) => host.reportError('Frame error', err);
frameLoop.wake(4);

// The user's ROMs, their favourites and their artwork all come back from IndexedDB.
// Asynchronously, so first paint is never blocked on a database transaction: the four
// bundled carts are already on screen and imports appear a moment later.
void (async () => {
  await importer.restoreLibrary();

  // Anything still without a cover gets one lookup, on idle, well after the library is
  // interactive. `boxart.js` remembers misses for a week, so this is silent and free on
  // every subsequent boot rather than a burst of 404s each time the app opens.
  const idle = (fn) =>
    'requestIdleCallback' in window
      ? requestIdleCallback(fn, { timeout: 4000 })
      : setTimeout(fn, 2000);
  idle(() => {
    const indices = allIndices();
    const entries = Array.from(indices, (index) => entryAt(index));
    void importer.backfillArtwork(entries);
  });
})();

// The save-state index is hydrated the same way: metadata only, so this reads a few
// kilobytes rather than the megabytes of payload sitting behind it. Once it resolves,
// `listFor()` answers synchronously, which is what lets the virtualised state list
// bind rows on the scroll path.
void (async () => {
  // Cheat lists come back the same way, and for the same reason: the UI needs to answer
  // "does this game have cheats" synchronously while rendering.
  const cheatSummary = await cheatStore.hydrate();
  if (cheatSummary.cheats > 0) {
    console.info(
      `[cheats] ${cheatSummary.cheats} cheat(s) across ${cheatSummary.games} game(s) restored`,
    );
  }

  const { states, games } = await saveStates.hydrate();
  if (states > 0) {
    console.info(`[states] ${states} saved state(s) across ${games} game(s) restored`);
    library.refreshData();
    if (detail.isOpen) detail.reloadStates();
  }

  // Asked for once, and only informational: iOS clears non-persistent storage for
  // sites left unvisited, which for save states means losing progress to inactivity.
  // Granted silently for installed PWAs, usually refused for a plain tab.
  const persistent = await requestPersistentStorage();
  const estimate = await storageEstimate();
  console.info(
    `[storage] persistent: ${persistent}` +
      (estimate
        ? ` · using ${(estimate.usage / 1048576).toFixed(1)} MB of ` +
          `${(estimate.quota / 1048576).toFixed(0)} MB`
        : ''),
  );
})();

// Checkpointing the running game. `visibilitychange → hidden` is the load-bearing one
// on iOS: `pagehide` and `beforeunload` are unreliable there, and a storage write
// started during teardown may never commit. Backgrounding fires while the page is
// still alive, so the transaction has time to finish.
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') void player.autoSave('hidden', { force: true });
});
// Belt and braces for desktop browsers, where this does fire.
window.addEventListener('pagehide', () => void player.autoSave('pagehide', { force: true }));

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
  // Whether a worker was already in charge when this page loaded. On a first-ever
  // visit the worker claims the page a moment later and `controllerchange` fires
  // once — that is not an update, and reloading for it would be a pointless flash.
  const hadController = Boolean(navigator.serviceWorker.controller);

  window.addEventListener('load', () => {
    navigator.serviceWorker.register('./sw.js').catch((err) => {
      // Offline support is a bonus; the app works fine without it.
      console.warn('[pwa] service worker registration failed', err);
    });
  });

  // A new deploy means a new worker, which purges the old shell cache on activate.
  // The page you are looking at, though, was already built from the *previous*
  // cache — so without this you would see stale code on the first reload after a
  // deploy and the new code only on the second. That is a genuinely confusing way
  // to test a change.
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (!hadController) return;
    // Never yank the page out from under a running game.
    if (player?.active || host.bridge?.status === 'running') {
      toast('Update installed', 'Exit to the library and reload to pick it up.', {
        kind: 'info',
        ms: 10000,
      });
      return;
    }
    // Guard against a reload loop if a worker ever activates repeatedly.
    if (sessionStorage.getItem('continuum:reloading-for-update')) return;
    sessionStorage.setItem('continuum:reloading-for-update', '1');
    location.reload();
  });

  window.addEventListener('load', () => {
    sessionStorage.removeItem('continuum:reloading-for-update');
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
  settings,
  cheatSheet,
  coreOptionsSheet,
  touchEditor,
  player,
  importer,
  coreMenu,
  frameLoop,
  launch,
  corePrefs: { get: getCorePreference, set: setCorePreference },
};
