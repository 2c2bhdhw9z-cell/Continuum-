/**
 * Service worker: offline shell.
 *
 * Scope is deliberately narrow. Two things it does *not* do:
 *
 *   - **Core binaries.** Requests under `cores/` are passed straight through.
 *     `core-loader.js` manages those in its own versioned Cache bucket, because
 *     core lifetime is a user-facing concern ("free up space") that should not be
 *     entangled with shell versioning.
 *   - **Range requests.** Anything with a `Range` header bypasses the cache
 *     entirely; a partial response cached as complete is a corruption bug that
 *     surfaces much later, when large content streaming lands in Phase 2.
 */

const VERSION = 'v1';
const SHELL_CACHE = `continuum-shell-${VERSION}`;

/** The minimum needed for a cold offline start. */
const SHELL_ASSETS = [
  './',
  './index.html',
  './manifest.webmanifest',
  './icons/icon.svg',
  './styles/tokens.css',
  './styles/base.css',
  './styles/shell.css',
  './styles/library.css',
  './styles/player.css',
  './src/main.js',
  './src/engine/loop.js',
  './src/engine/bridge-host.js',
  './src/engine/core-loader.js',
  './src/engine/input.js',
  './src/engine/core-runtime.js',
  './src/audio/audio-output.js',
  './src/audio/pcm-worklet.js',
  './src/ui/virtual-scroller.js',
  './src/ui/library-view.js',
  './src/ui/detail-sheet.js',
  './src/ui/player-view.js',
  './src/ui/card.js',
  './src/ui/art.js',
  './src/ui/toast.js',
  './src/data/catalog.js',
  './src/data/systems.js',
  './src/data/save-states.js',
  './src/data/content-store.js',
  './src/data/rom-store.js',
  './src/data/rom-detect.js',
  './src/data/builtins.js',
  './src/ui/rom-import.js',
  './roms/nes-testcart.nes',
  './vendor/bridge/emulator_bridge.js',
  './vendor/bridge/emulator_bridge_bg.wasm',
  './cores/manifest.json',
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(SHELL_CACHE);
      // Individually, not addAll: one 404 during development should not fail the
      // entire install and leave the app without a worker.
      await Promise.all(
        SHELL_ASSETS.map(async (url) => {
          try {
            await cache.add(new Request(url, { cache: 'reload' }));
          } catch (err) {
            console.warn('[sw] could not precache', url, err);
          }
        }),
      );
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(
        names
          .filter((name) => name.startsWith('continuum-shell-') && name !== SHELL_CACHE)
          .map((name) => caches.delete(name)),
      );
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  // Core binaries and partial content: not ours to manage.
  if (url.pathname.includes('/cores/') && url.pathname.endsWith('.wasm')) return;
  if (request.headers.has('range')) return;

  // Navigations: network first so a deploy is picked up immediately, cache as the
  // offline fallback.
  if (request.mode === 'navigate') {
    event.respondWith(
      (async () => {
        try {
          return await fetch(request);
        } catch {
          const cache = await caches.open(SHELL_CACHE);
          return (
            (await cache.match('./index.html')) ??
            new Response('Offline', { status: 503, statusText: 'Offline' })
          );
        }
      })(),
    );
    return;
  }

  // Assets: cache first, then refresh in the background (stale-while-revalidate).
  event.respondWith(
    (async () => {
      const cache = await caches.open(SHELL_CACHE);
      const cached = await cache.match(request);
      const network = fetch(request)
        .then((response) => {
          if (response.ok) void cache.put(request, response.clone());
          return response;
        })
        .catch(() => null);

      if (cached) return cached;
      const fresh = await network;
      return fresh ?? new Response('Offline', { status: 503, statusText: 'Offline' });
    })(),
  );
});
