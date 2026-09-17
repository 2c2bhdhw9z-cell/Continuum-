/**
 * FrameLoop — the app's only `requestAnimationFrame` loop.
 *
 * Architectural rule: one loop drives everything. Per frame, in order:
 *
 *   1. engine tick (input → core step → audio → GPU present), when a game runs;
 *   2. audio pump, moving PCM from the bridge's ring to the output device;
 *   3. UI flush tasks — the virtual scrollers reconciling their windows.
 *
 * Ordering matters. The emulator gets the frame's budget first, because a late
 * present is a visible stutter while a late shelf re-render is imperceptible. And
 * because scrolling is flushed from the same loop rather than from its own rAF,
 * a burst of scroll events cannot starve emulation.
 *
 * ## Idling
 *
 * A permanently spinning rAF would drain battery while the user reads their
 * library. The loop instead stops when there is nothing to do and is woken by
 * whoever creates work (`wake()`), staying awake for a few extra frames to cover
 * momentum scrolling.
 */

/** Frames to keep spinning after the last `wake()`. Covers scroll inertia. */
const DEFAULT_WAKE_FRAMES = 3;

export class FrameLoop {
  constructor() {
    /** @type {((now: number) => void) | null} */
    this.engineTick = null;
    this.engineActive = false;
    /** @type {Array<(now: number) => void>} */
    this.flushTasks = [];
    this.rafId = 0;
    this.wakeFrames = 0;
    this.running = false;

    // Rolling diagnostics for the status bar / HUD.
    this.frameCount = 0;
    this.lastFrameMs = 0;
    this.worstFrameMs = 0;

    /** @type {(err: unknown) => void} */
    this.onError = (err) => console.error('[loop] frame error', err);

    this._frame = this._frame.bind(this);
  }

  /** Installs the engine step. Called once the wasm bridge is live. */
  setEngineTick(fn) {
    this.engineTick = fn;
  }

  /** Starts/stops emulation. While active, the loop never idles. */
  setEngineActive(active) {
    this.engineActive = !!active;
    if (this.engineActive) this._schedule();
  }

  /** Registers a per-frame task (a virtual scroller's `flush`). */
  addFlushTask(fn) {
    if (!this.flushTasks.includes(fn)) this.flushTasks.push(fn);
  }

  removeFlushTask(fn) {
    const i = this.flushTasks.indexOf(fn);
    if (i >= 0) this.flushTasks.splice(i, 1);
  }

  /**
   * Requests `frames` more frames of processing. Called by anything that creates
   * work: scroll events, resizes, view changes, data updates.
   */
  wake(frames = DEFAULT_WAKE_FRAMES) {
    this.wakeFrames = Math.max(this.wakeFrames, frames);
    this._schedule();
  }

  _schedule() {
    if (this.rafId === 0) {
      this.running = true;
      this.rafId = requestAnimationFrame(this._frame);
    }
  }

  _frame(now) {
    this.rafId = 0;
    const started = performance.now();

    // 1 + 2. Emulation and audio. Isolated: a core error must not take the UI
    // down with it, or the user cannot even navigate back to their library.
    if (this.engineActive && this.engineTick) {
      try {
        this.engineTick(now);
      } catch (err) {
        this.onError(err);
      }
    }

    // 3. UI. Each task is independent, so one failure must not skip the rest.
    for (let i = 0; i < this.flushTasks.length; i++) {
      try {
        this.flushTasks[i](now);
      } catch (err) {
        this.onError(err);
      }
    }

    this.frameCount++;
    this.lastFrameMs = performance.now() - started;
    if (this.lastFrameMs > this.worstFrameMs) this.worstFrameMs = this.lastFrameMs;

    if (this.engineActive) {
      this._schedule();
    } else if (this.wakeFrames > 0) {
      this.wakeFrames--;
      this._schedule();
    } else {
      this.running = false;
    }
  }

  stop() {
    if (this.rafId !== 0) cancelAnimationFrame(this.rafId);
    this.rafId = 0;
    this.running = false;
    this.engineActive = false;
  }
}

/** The single shared instance. Importing a second loop would break the rule. */
export const frameLoop = new FrameLoop();
