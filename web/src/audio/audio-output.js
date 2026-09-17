/**
 * Web Audio output: the consumer end of the Rust `AudioSink`.
 *
 * ```text
 *   Rust ring ──drainAudioInto──▶ wasm staging ──copy──▶ pooled ArrayBuffer
 *        ──transfer──▶ AudioWorklet ring ──▶ AudioContext ──▶ speakers
 * ```
 *
 * ## Autoplay policy (the part that silently breaks on iOS)
 *
 * Safari — and Chrome, less strictly — will not start an `AudioContext` outside a
 * user gesture. A context created during page load starts `suspended`, and every
 * sample pushed into it disappears with no error anywhere. That failure mode is
 * indistinguishable from a broken audio pipeline, so this module treats the unlock
 * as a first-class state:
 *
 *   1. The context is *created* lazily, inside a gesture (`ensureStarted`), never
 *      at boot.
 *   2. `installGestureUnlock` listens for `pointerdown` / `touchend` / `keydown`
 *      and resumes. The listeners stay attached rather than firing once, because
 *      iOS re-suspends after interruptions (an incoming call, the ringer switch, or
 *      the tab going to the background).
 *   3. `state` is observable, and the player shows a "tap to enable sound" button
 *      whenever it is not `running`. Better to ask than to play silently.
 *
 * ## Draining while muted
 *
 * If the device is not consuming, the Rust ring still needs emptying — otherwise it
 * saturates and reports overruns that look like a bug. So `pump()` always drains;
 * it just discards when there is nowhere to send.
 */

/** Worklet ring size, in frames. ~170 ms at 48 kHz: absorbs a hitch without lag. */
const WORKLET_RING_FRAMES = 8192;

/** Samples per transferred chunk. 4096 = 2048 stereo frames, ~42 ms at 48 kHz. */
const CHUNK_SAMPLES = 4096;

/** Pooled transfer buffers. Three in flight is plenty at 60 Hz. */
const POOL_SIZE = 3;

export class AudioOutput {
  /** @param {import('../engine/bridge-host.js').BridgeHost} host */
  constructor(host) {
    this.host = host;
    /** @type {AudioContext|null} */
    this.ctx = null;
    /** @type {AudioWorkletNode|null} */
    this.node = null;
    /** @type {GainNode|null} */
    this.gain = null;

    /** 'idle' | 'starting' | 'running' | 'suspended' | 'unsupported' | 'failed' */
    this.state = 'idle';
    this.muted = false;
    this.lastError = null;

    /** Stats reported back by the worklet. */
    this.workletStats = { queuedFrames: 0, capacityFrames: 0, underruns: 0, overruns: 0 };

    /** @type {ArrayBuffer[]} */
    this._pool = [];
    this._poolMisses = 0;
    this._startPromise = null;
    /** @type {(state: string) => void} */
    this.onStateChange = () => {};
  }

  get sampleRate() {
    return this.ctx?.sampleRate ?? 0;
  }

  get isRunning() {
    return this.state === 'running';
  }

  /** Milliseconds of audio buffered in the worklet. Surfaced in the HUD. */
  get bufferedMs() {
    if (!this.sampleRate) return 0;
    return (this.workletStats.queuedFrames / this.sampleRate) * 1000;
  }

  /**
   * Creates the graph. **Must be called from a user gesture** the first time.
   * Idempotent; concurrent calls share one promise.
   */
  async ensureStarted() {
    if (this.state === 'running') return;
    if (this._startPromise) return this._startPromise;

    this._startPromise = (async () => {
      const Ctor = window.AudioContext ?? window.webkitAudioContext;
      if (!Ctor) {
        this.state = 'unsupported';
        this.onStateChange(this.state);
        return;
      }

      this.state = 'starting';
      this.onStateChange(this.state);

      try {
        if (!this.ctx) {
          // `interactive` asks for the smallest buffer the platform will give,
          // which is what an emulator wants: latency is felt as input lag.
          this.ctx = new Ctor({ latencyHint: 'interactive' });
          this.ctx.onstatechange = () => this._syncState();
        }

        if (!this.ctx.audioWorklet) {
          // Very old Safari. Rather than ship a ScriptProcessorNode fallback that
          // runs on the main thread and stutters, report it honestly.
          this.state = 'unsupported';
          this.lastError = new Error('AudioWorklet is not supported in this browser');
          this.onStateChange(this.state);
          return;
        }

        if (!this.node) {
          await this.ctx.audioWorklet.addModule(
            new URL('./pcm-worklet.js', import.meta.url),
          );
          this.node = new AudioWorkletNode(this.ctx, 'pcm-queue', {
            numberOfInputs: 0,
            numberOfOutputs: 1,
            outputChannelCount: [2],
            processorOptions: { ringFrames: WORKLET_RING_FRAMES },
          });
          this.node.port.onmessage = (event) => this._onWorkletMessage(event);

          this.gain = this.ctx.createGain();
          this.gain.gain.value = this.muted ? 0 : 1;
          this.node.connect(this.gain).connect(this.ctx.destination);

          for (let i = 0; i < POOL_SIZE; i++) {
            this._pool.push(new ArrayBuffer(CHUNK_SAMPLES * 4));
          }
        }

        if (this.ctx.state === 'suspended') await this.ctx.resume();

        // The device rate is only knowable now, and the Rust resampler needs it.
        this.host.bridge?.setOutputSampleRate(Math.round(this.ctx.sampleRate));
        this._syncState();
        console.info(
          `[audio] context running at ${Math.round(this.ctx.sampleRate)} Hz, ` +
            `base latency ${((this.ctx.baseLatency ?? 0) * 1000).toFixed(1)} ms`,
        );
      } catch (err) {
        this.state = 'failed';
        this.lastError = err;
        console.error('[audio] failed to start', err);
        this.onStateChange(this.state);
      } finally {
        this._startPromise = null;
      }
    })();

    return this._startPromise;
  }

  /**
   * Attaches gesture listeners that unlock (or re-unlock) audio.
   *
   * Not `{ once: true }` on purpose: iOS suspends the context again after
   * interruptions, and the next tap should recover without a reload.
   */
  installGestureUnlock() {
    const unlock = () => {
      if (this.state === 'unsupported' || this.state === 'failed') return;
      if (this.state === 'running' && this.ctx?.state === 'running') return;
      void this.resume();
    };

    for (const type of ['pointerdown', 'touchend', 'keydown']) {
      window.addEventListener(type, unlock, { capture: true, passive: true });
    }

    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        // Let the OS reclaim the audio device while backgrounded.
        void this.ctx?.suspend?.().catch(() => {});
      } else if (this.node) {
        void this.resume();
      }
    });
  }

  /** Resumes (creating the graph if needed). Safe to call from any gesture. */
  async resume() {
    if (!this.ctx || !this.node) {
      await this.ensureStarted();
      return;
    }
    try {
      await this.ctx.resume();
    } catch (err) {
      // Not fatal: it usually means this call was not gesture-adjacent after all.
      console.warn('[audio] resume rejected', err);
    }
    this._syncState();
  }

  async suspend() {
    if (!this.ctx) return;
    try {
      await this.ctx.suspend();
    } catch { /* already suspended */ }
    this._syncState();
  }

  setMuted(muted) {
    this.muted = muted;
    if (this.gain && this.ctx) {
      // Ramp instead of a step: an instant gain change is an audible click.
      const now = this.ctx.currentTime;
      this.gain.gain.cancelScheduledValues(now);
      this.gain.gain.setTargetAtTime(muted ? 0 : 1, now, 0.015);
    }
  }

  /** Drops queued audio on both sides. Used on reset and load-state. */
  flush() {
    this.node?.port.postMessage({ type: 'flush' });
  }

  /**
   * Moves available PCM from the bridge to the device. Called once per engine tick,
   * after the core step, as the "Audio" stage of the unified loop.
   */
  pump() {
    const bridge = this.host.bridge;
    if (!bridge) return;

    if (!this.node || this.state !== 'running') {
      // Keep the Rust ring from saturating while audio is locked or muted;
      // otherwise its overrun counter fills up with a non-problem.
      this.host.drainAudio(CHUNK_SAMPLES);
      return;
    }

    // Bounded: a pathological backlog must not turn one frame into a long loop.
    for (let iteration = 0; iteration < 4; iteration++) {
      const buffer = this._takeBuffer();
      const view = new Float32Array(buffer);
      const samples = this.host.drainAudio(view.length);
      if (samples === 0) {
        this._returnBuffer(buffer);
        return;
      }

      view.set(this.host.audioView.subarray(0, samples));
      this.node.port.postMessage(
        { type: 'pcm', buffer, length: samples },
        [buffer], // Transferred: no structured-clone copy.
      );

      if (samples < view.length) return; // Ring drained.
    }
  }

  _takeBuffer() {
    const pooled = this._pool.pop();
    if (pooled) return pooled;
    // Pool exhausted because buffers are still in flight. Allocating is better
    // than dropping audio; log once so a systematic leak is visible.
    if (this._poolMisses++ === 0) {
      console.debug('[audio] transfer pool exhausted; allocating an extra buffer');
    }
    return new ArrayBuffer(CHUNK_SAMPLES * 4);
  }

  _returnBuffer(buffer) {
    if (buffer.byteLength > 0 && this._pool.length < POOL_SIZE + 2) {
      this._pool.push(buffer);
    }
  }

  _onWorkletMessage(event) {
    const data = event.data;
    if (!data) return;
    if (data.type === 'recycle') {
      this._returnBuffer(data.buffer);
      return;
    }
    if (data.type === 'stats') {
      this.workletStats.queuedFrames = data.queuedFrames;
      this.workletStats.capacityFrames = data.capacityFrames;
      this.workletStats.underruns = data.underruns;
      this.workletStats.overruns = data.overruns;
    }
  }

  _syncState() {
    if (!this.ctx) return;
    const next =
      this.ctx.state === 'running'
        ? 'running'
        : this.ctx.state === 'suspended'
          ? 'suspended'
          : 'idle';
    if (next !== this.state) {
      this.state = next;
      this.onStateChange(this.state);
    }
  }
}
