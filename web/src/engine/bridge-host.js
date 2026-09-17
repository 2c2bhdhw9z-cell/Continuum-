/**
 * Host side of the Rust bridge.
 *
 * Owns the wasm module's lifetime and the zero-copy views into its memory. Every
 * other JS module talks to the emulator through this object, never through the
 * generated glue directly — so when Phase 2 replaces the wasm facade with a native
 * one, this file is the only thing that changes shape.
 *
 * ## Views over wasm memory
 *
 * `tick()` writes telemetry into a fixed block inside wasm memory, and
 * `drainAudioInto()` fills an audio staging block there. Both are read through
 * typed-array views created once. The alternative — returning objects or passing
 * `Float32Array`s in — allocates on every frame at 60 Hz.
 *
 * Views are invalidated only by wasm memory growth, which detaches their
 * `ArrayBuffer`. `_ensureViews` detects that (`byteLength === 0`) and rebuilds.
 * Cheap enough to check per frame, and a use-after-detach is otherwise a silent
 * source of zeros.
 */

import { toast } from '../ui/toast.js';

/** Device pixel ratio cap. Beyond 2x the fill cost buys nothing visible. */
const MAX_DPR = 2;

export class BridgeHost {
  constructor() {
    /** @type {any} The wasm-bindgen module namespace. */
    this.wasm = null;
    /** @type {any} The `EmulatorBridge` instance. */
    this.bridge = null;
    /** @type {WebAssembly.Memory|null} */
    this.memory = null;

    this.telemetryView = null;
    this.telemetryLayout = null;
    this.audioView = null;

    this._loadPromise = null;
    this._gpuReady = false;
    this._canvas = null;
    this._resizeObserver = null;

    /**
     * Reused telemetry snapshot. Mutated in place each frame so reading stats
     * costs no allocation.
     */
    this.stats = {
      steps: 0,
      dropped: 0,
      presented: false,
      resynced: false,
      displayFps: 0,
      frameCount: 0,
      audioQueuedFrames: 0,
      audioCapacityFrames: 0,
      audioOverruns: 0,
      audioUnderruns: 0,
      audioFramesSubmitted: 0,
      audioFramesDrained: 0,
    };
  }

  /** True when the browser exposes WebGPU at all. Checked before any wasm work. */
  static get webgpuAvailable() {
    return typeof navigator !== 'undefined' && 'gpu' in navigator;
  }

  /**
   * Loads and initialises the wasm bridge. Idempotent and safe to call
   * concurrently — the first call wins and the rest await it.
   *
   * Note this loads the *engine*, not any emulator core: the registry starts empty
   * and cores arrive only when a game is launched.
   */
  async load() {
    if (this._loadPromise) return this._loadPromise;
    this._loadPromise = (async () => {
      const wasm = await import('../../vendor/bridge/emulator_bridge.js');
      const output = await wasm.default();
      this.wasm = wasm;
      this.memory = output.memory;
      this.bridge = new wasm.EmulatorBridge();

      this.telemetryLayout = wasm.EmulatorBridge.telemetryLayout();
      this._ensureViews(true);

      console.info(
        '[bridge] engine ready; telemetry slots:',
        Object.keys(this.telemetryLayout).length,
      );
      return this.bridge;
    })();
    return this._loadPromise;
  }

  get isLoaded() {
    return this.bridge !== null;
  }

  get isGpuReady() {
    return this._gpuReady;
  }

  _ensureViews(force = false) {
    if (!this.bridge || !this.memory) return;
    const buffer = this.memory.buffer;
    const detached = !this.telemetryView || this.telemetryView.buffer.byteLength === 0;
    if (!force && !detached && this.telemetryView.buffer === buffer) return;

    this.telemetryView = new Float64Array(
      buffer,
      this.bridge.telemetryPtr,
      this.bridge.telemetryLen,
    );
    this.audioView = new Float32Array(
      buffer,
      this.bridge.audioBufferPtr,
      this.bridge.audioBufferLen,
    );
  }

  /**
   * Initialises WebGPU against `canvas`.
   *
   * The canvas must be visible and laid out: a `display: none` element measures
   * zero, and a zero-sized surface configuration is a validation error.
   */
  async initGpu(canvas) {
    await this.load();
    if (this._gpuReady) return;

    if (!BridgeHost.webgpuAvailable) {
      throw new Error(
        'WebGPU is unavailable. Chrome/Edge 113+, Safari 18+ or Firefox 141+ is required; ' +
          'there is no WebGL fallback in this build by design.',
      );
    }

    this._canvas = canvas;
    const { width, height } = this._physicalSize(canvas);
    await this.bridge.initGpu(canvas, width, height);
    this._gpuReady = true;

    // wgpu owns canvas.width/height; only the CSS box is ours, so all we do here
    // is forward the new physical size.
    this._resizeObserver = new ResizeObserver(() => this.syncCanvasSize());
    this._resizeObserver.observe(canvas);

    console.info('[bridge] GPU adapter:', this.bridge.adapterInfo);
  }

  _physicalSize(canvas) {
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    const rect = canvas.getBoundingClientRect();
    return {
      width: Math.max(1, Math.round((rect.width || canvas.clientWidth || 1) * dpr)),
      height: Math.max(1, Math.round((rect.height || canvas.clientHeight || 1) * dpr)),
    };
  }

  /** Reconfigures the swapchain after a layout change. No-op when unchanged. */
  syncCanvasSize() {
    if (!this._gpuReady || !this._canvas) return;
    const { width, height } = this._physicalSize(this._canvas);
    this.bridge.resize(width, height);
  }

  /**
   * Runs one engine tick and refreshes `stats`.
   * @param {number} nowMs The rAF timestamp.
   */
  tick(nowMs) {
    if (!this.bridge) return this.stats;
    this.bridge.tick(nowMs);
    this._ensureViews();

    const t = this.telemetryView;
    const l = this.telemetryLayout;
    const s = this.stats;
    s.steps = t[l.steps];
    s.dropped = t[l.dropped];
    s.presented = t[l.presented] !== 0;
    s.resynced = t[l.resynced] !== 0;
    s.displayFps = t[l.displayFps];
    s.frameCount = t[l.frameCount];
    s.audioQueuedFrames = t[l.audioQueuedFrames];
    s.audioCapacityFrames = t[l.audioCapacityFrames];
    s.audioOverruns = t[l.audioOverruns];
    s.audioUnderruns = t[l.audioUnderruns];
    s.audioFramesSubmitted = t[l.audioFramesSubmitted];
    s.audioFramesDrained = t[l.audioFramesDrained];
    return s;
  }

  /**
   * Reads the presented image back from the GPU as tightly packed RGBA8.
   *
   * Zero dimensions mean the current canvas size. Costs a texture, a staging
   * buffer and a buffer map, so it belongs in screenshot and thumbnail paths — not
   * in the frame loop.
   *
   * @param {number} [width]
   * @param {number} [height]
   * @returns {Promise<Uint8Array>}
   */
  async captureFrame(width = 0, height = 0) {
    if (!this.bridge) throw new Error('bridge is not loaded');
    if (!this._gpuReady) throw new Error('GPU is not initialised');
    return this.bridge.captureFrame(width, height);
  }

  /**
   * Drains queued PCM into wasm-side staging.
   * @param {number} maxSamples
   * @returns {number} samples available in `audioView`.
   */
  drainAudio(maxSamples) {
    if (!this.bridge) return 0;
    const n = this.bridge.drainAudioInto(maxSamples);
    if (n > 0) this._ensureViews();
    return n;
  }

  /** Reports an engine error to the user without killing the UI. */
  reportError(context, error) {
    console.error(`[bridge] ${context}`, error);
    toast(context, String(error?.message ?? error), { kind: 'error', ms: 6000 });
  }

  destroy() {
    this._resizeObserver?.disconnect();
    this._resizeObserver = null;
  }
}
