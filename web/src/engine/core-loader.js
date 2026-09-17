/**
 * Dynamic core loading.
 *
 * Architectural rule: **no emulator core is fetched or instantiated at boot.** At
 * startup this module reads `cores/manifest.json` — a few KB of metadata — and
 * declares each core to the Rust registry. Declaring costs a descriptor and
 * nothing else: no network request for the module, no wasm instantiation, no
 * memory.
 *
 * A core binary is fetched exactly once, the first time a game that needs it is
 * launched:
 *
 *   manifest (boot) → declare → [user launches a game] → fetch + progress
 *   → Cache API → attachCoreModule → session
 *
 * Later launches of the same system skip everything: the registry keeps the core
 * resident, and even after `unloadCore` the bytes are served from the Cache API
 * rather than the network.
 *
 * Two kinds of entry exist. `kind: "libretro"` is a real core, instantiated here as
 * its own wasm module and handed to Rust as a runtime handle. Anything else is a
 * placeholder whose bytes go to Rust, which substitutes its diagnostic core. The
 * registry treats both identically, which is what made adding real cores a
 * one-file change.
 */

import { LibretroRuntime } from './core-runtime.js';

/** Cache bucket for core binaries. Versioned so a format change can invalidate it. */
const CORE_CACHE = 'continuum-cores-v1';

export class CoreLoader {
  /** @param {import('./bridge-host.js').BridgeHost} host */
  constructor(host) {
    this.host = host;
    /** @type {Map<string, any>} coreId -> manifest entry */
    this.manifest = new Map();
    /** @type {Map<string, string>} systemId -> coreId */
    this.systemToCore = new Map();
    /** @type {Map<string, Promise<void>>} In-flight fetches, deduped by core id. */
    this.inFlight = new Map();

    /**
     * Cached typed-array views into *Rust* memory for the frame hand-off.
     *
     * The staging pointer is stable for a session and the core's framebuffer pointer
     * rarely moves, so these are built once and reused. Allocating a view per frame
     * would put 60 short-lived objects a second in front of the collector for no
     * reason.
     */
    this._videoDst = null;
    this._videoDstKey = '';
    this._audioDst = null;
    this._audioDstKey = '';
  }

  /**
   * Fetches the manifest and declares every core to the bridge.
   * @param {string} [url]
   */
  async loadManifest(url = './cores/manifest.json') {
    const bridge = await this.host.load();
    const response = await fetch(url, { cache: 'no-cache' });
    if (!response.ok) {
      throw new Error(`core manifest ${response.status} ${response.statusText}`);
    }
    const data = await response.json();

    for (const core of data.cores ?? []) {
      this.manifest.set(core.id, core);
      for (const systemId of core.systems ?? []) {
        // First core wins; a future settings screen can override per system.
        if (!this.systemToCore.has(systemId)) this.systemToCore.set(systemId, core.id);
      }

      // Metadata only. This is the line that keeps boot cheap.
      const declaration = new this.host.wasm.CoreDeclaration(
        core.id,
        core.name,
        (core.systems ?? []).join(','),
        core.module,
        core.geometry.baseWidth,
        core.geometry.baseHeight,
        core.geometry.aspectRatio,
        core.targetFps,
        core.audioSampleRate,
        pixelFormatId(core.pixelFormat),
      );
      const maxW = core.geometry.maxWidth ?? core.geometry.baseWidth;
      const maxH = core.geometry.maxHeight ?? core.geometry.baseHeight;
      bridge.declareCore(declaration.withMaxGeometry(maxW, maxH));
    }

    console.info(
      `[cores] declared ${this.manifest.size} cores, 0 loaded (${bridge.residentCoreCount} resident)`,
    );
    return this.manifest;
  }

  coreIdFor(systemId) {
    return this.systemToCore.get(systemId) ?? null;
  }

  entryFor(coreId) {
    return this.manifest.get(coreId) ?? null;
  }

  /** Whether a core's module is already instantiated in the registry. */
  isResident(coreId) {
    const state = this.host.bridge?.coreState(coreId);
    return state === 'loaded' || state === 'bound';
  }

  /**
   * Ensures `coreId` is loaded and runnable.
   * @param {string} coreId
   * @param {(progress: {received: number, total: number, phase: string}) => void} [onProgress]
   */
  async ensureCore(coreId, onProgress = () => {}) {
    const bridge = await this.host.load();
    if (this.isResident(coreId)) {
      onProgress({ received: 1, total: 1, phase: 'ready' });
      return;
    }

    // Two launches racing for the same core must produce one download.
    const existing = this.inFlight.get(coreId);
    if (existing) return existing;

    const entry = this.manifest.get(coreId);
    if (!entry) throw new Error(`core '${coreId}' is not in the manifest`);

    const task = (async () => {
      onProgress({ received: 0, total: entry.sizeBytes ?? 0, phase: 'fetch' });
      const bytes = await this._fetchModule(entry, onProgress);

      onProgress({ received: bytes.length, total: bytes.length, phase: 'instantiate' });

      if (entry.kind === 'libretro') {
        // A real core is its own wasm module with its own memory and imports, which
        // only JS can wire up — so instantiation happens here and Rust receives a
        // handle. Everything after this line (pacing, input, audio, presentation) is
        // Rust's.
        const runtime = await LibretroRuntime.instantiate(bytes, this._coreHooks());
        bridge.attachCoreRuntime(coreId, runtime);
        console.info(
          `[cores] '${coreId}' instantiated: ${runtime.systemInfo.name} ` +
            `${runtime.systemInfo.version} (${runtime.systemInfo.validExtensions.join('/')})`,
        );
      } else {
        // Placeholder entries: Rust validates the module and substitutes its
        // diagnostic core. Same registry, same session lifecycle.
        bridge.attachCoreModule(coreId, bytes);
      }

      console.info(
        `[cores] '${coreId}' attached (${bytes.length} bytes); resident: ${bridge.residentCoreCount}`,
      );
    })();

    this.inFlight.set(coreId, task);
    try {
      await task;
    } finally {
      this.inFlight.delete(coreId);
    }
  }

  /**
   * Hooks handed to a `LibretroRuntime`, wiring the core's callbacks to Rust.
   *
   * This is the whole JS contribution to a running frame: copy the core's framebuffer
   * and PCM into the staging buffers Rust published, then tell Rust what arrived. No
   * decisions are made here — not the pixel format, not the geometry, not the input
   * mapping — because every one of those would then have to be re-made in Swift for
   * Phase 2.
   */
  _coreHooks() {
    const CoreHost = this.host.wasm.CoreHost;

    return {
      video: ({ data, width, height, pitch, format }) => {
        const ptr = CoreHost.videoStagingPtr();
        // Zero means no frame window is open, which happens if a core calls
        // video_refresh outside retro_run. Rust logs it; dropping is correct.
        if (!ptr) return;

        const needed = pitch * height;
        const capacity = CoreHost.videoStagingLen();
        if (needed > capacity) {
          // Report it anyway so Rust counts the drop and warns once with real numbers.
          CoreHost.videoReady(width, height, pitch, format);
          return;
        }

        const key = `${ptr}:${capacity}:${this.host.memory.buffer.byteLength}`;
        if (key !== this._videoDstKey) {
          this._videoDst = new Uint8Array(this.host.memory.buffer, ptr, capacity);
          this._videoDstKey = key;
        }
        this._videoDst.set(data.subarray(0, needed));
        CoreHost.videoReady(width, height, pitch, format);
      },

      audio: (samples, frames) => {
        const ptr = CoreHost.audioStagingPtr();
        if (!ptr) return;

        const capacity = CoreHost.audioStagingLen();
        const key = `${ptr}:${capacity}:${this.host.memory.buffer.byteLength}`;
        if (key !== this._audioDstKey) {
          this._audioDst = new Int16Array(this.host.memory.buffer, ptr, capacity);
          this._audioDstKey = key;
        }

        const count = Math.min(samples.length, capacity);
        this._audioDst.set(count === samples.length ? samples : samples.subarray(0, count));
        CoreHost.audioReady(count >> 1);
      },

      // Rust owns the input state, so the core's query goes straight to it.
      inputState: (port, device, index, id) => CoreHost.inputState(port, device, index, id),

      systemChanged: ({ geometry }) => {
        if (geometry) {
          CoreHost.geometryChanged(
            geometry.baseWidth,
            geometry.baseHeight,
            geometry.aspectRatio,
          );
        }
      },

      log: (message) => console.debug(message),
    };
  }

  /**
   * Fetches a core module, preferring the cache, and reports progress.
   * @returns {Promise<Uint8Array>}
   */
  async _fetchModule(entry, onProgress) {
    const url = new URL(entry.module, document.baseURI).href;

    // Cache API rather than the HTTP cache: cores are large, immutable and worth
    // keeping across sessions independently of the service worker's shell cache.
    let cache = null;
    if ('caches' in window) {
      try {
        cache = await caches.open(CORE_CACHE);
        const hit = await cache.match(url);
        if (hit) {
          onProgress({ received: 0, total: 0, phase: 'cache' });
          const buffer = await hit.arrayBuffer();
          return new Uint8Array(buffer);
        }
      } catch (err) {
        // A blocked Cache API (private mode, some embeds) must not break loading.
        console.warn('[cores] cache unavailable, falling back to network', err);
      }
    }

    const response = await fetch(url);
    if (!response.ok) {
      // Core binaries are build outputs, not repository contents (they are large, and
      // third-party GPL code we do not vendor). A 404 almost always means "not built
      // yet", so say so instead of reporting a bare status code.
      const hint =
        response.status === 404 && entry.kind === 'libretro'
          ? ` — build it with: scripts/build-core.sh ${entry.id}`
          : '';
      throw new Error(
        `core download failed: ${response.status} ${response.statusText}${hint}`,
      );
    }

    if (cache) {
      // Cache the untouched response before consuming the body.
      try {
        await cache.put(url, response.clone());
      } catch (err) {
        console.warn('[cores] could not cache core', err);
      }
    }

    const total = Number(response.headers.get('content-length') ?? entry.sizeBytes ?? 0);

    // Stream so the progress bar reflects reality on a slow connection. Without a
    // body reader the UI can only show an indeterminate spinner.
    if (!response.body) {
      const buffer = await response.arrayBuffer();
      return new Uint8Array(buffer);
    }

    const reader = response.body.getReader();
    const chunks = [];
    let received = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
      received += value.length;
      onProgress({ received, total, phase: 'fetch' });
    }

    const bytes = new Uint8Array(received);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    return bytes;
  }

  /** Clears cached core binaries. Exposed for a future settings screen. */
  async clearCache() {
    if (!('caches' in window)) return;
    await caches.delete(CORE_CACHE);
  }
}

/** Maps the manifest's pixel-format name to the FFI's numeric encoding. */
function pixelFormatId(name) {
  switch (name) {
    case 'rgb565':
      return 0;
    case 'xrgb8888':
      return 1;
    case 'rgba8888':
      return 2;
    default:
      throw new Error(`unknown pixel format '${name}' in core manifest`);
  }
}
