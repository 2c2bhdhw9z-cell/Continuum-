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
 */

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
      // Hands the bytes to Rust, which validates and instantiates. Throws with a
      // legible reason if the download was an error page or a truncated file.
      bridge.attachCoreModule(coreId, bytes);

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
      throw new Error(`core download failed: ${response.status} ${response.statusText}`);
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
