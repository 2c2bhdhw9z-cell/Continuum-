/**
 * Player view: the WebGPU canvas, the HUD, and the launch sequence.
 *
 * ## The engine tick
 *
 * `tick()` is the function handed to the app's single `requestAnimationFrame` loop,
 * and it is the whole runtime in four lines:
 *
 *   1. `input.pollGamepads()` — the Gamepad API has no events, so it is sampled
 *      here, before emulation, so a press lands in the same frame it was made.
 *   2. `host.tick(now)` — one call into Rust which runs the paced core steps, feeds
 *      the audio sink, and presents through wgpu. Steps per tick are decided by the
 *      Rust pacer, not by JS.
 *   3. `audio.pump()` — moves the PCM Rust just produced to the output device.
 *   4. HUD, throttled: reading stats every frame would cost more than the emulation.
 *
 * ## Launch sequence
 *
 * Ordering here is not arbitrary:
 *
 *   - `audio.ensureStarted()` is kicked off *synchronously* while still inside the
 *     click that triggered the launch. Safari treats the gesture as expired after an
 *     `await`, so starting the context later would leave the session muted.
 *   - GPU init happens after the view is visible, because a `hidden` canvas measures
 *     zero and a zero-sized surface configuration is invalid.
 *   - The core is fetched only now — never at boot — and only if not already resident.
 */

import { getContent, contentFilename } from '../data/content-store.js';
import {
  storeCapture,
  CAPTURE_AFTER_FRAME,
  CAPTURE_WIDTH,
  CAPTURE_HEIGHT,
} from '../data/artwork.js';
import { getSetting, setSetting } from '../data/settings.js';
import * as cheatStore from '../data/cheats.js';
import * as coreOptionStore from '../data/core-options.js';
import { entryById, markPlayed } from '../data/catalog.js';
import { getSystem } from '../data/systems.js';
import { getCorePreference } from '../data/core-prefs.js';
import * as saveStates from '../data/save-states.js';
import { toast } from './toast.js';

/** Idle delay before the chrome fades out during play. */
const CHROME_IDLE_MS = 2600;

/** HUD refresh interval, in frames. ~4 Hz at 60 fps. */
const HUD_INTERVAL = 15;

export class PlayerView {
  /**
   * @param {object} opts
   * @param {import('../engine/bridge-host.js').BridgeHost} opts.host
   * @param {import('../engine/core-loader.js').CoreLoader} opts.coreLoader
   * @param {import('../audio/audio-output.js').AudioOutput} opts.audio
   * @param {import('../engine/input.js').InputManager} opts.input
   * @param {import('../engine/loop.js').FrameLoop} opts.loop
   * @param {() => void} opts.onExit
   */
  constructor({
    host,
    coreLoader,
    audio,
    input,
    loop,
    onExit,
    onRequestImport,
    onStatesChanged,
    onArtworkCaptured,
    onOpenCheats,
    onOpenCoreOptions,
    onEditLayout,
  }) {
    this.host = host;
    this.coreLoader = coreLoader;
    this.audio = audio;
    this.input = input;
    this.loop = loop;
    this.onExit = onExit;
    /** Called when the user asks to add a ROM from inside the player. */
    this.onRequestImport = onRequestImport;
    /** Called after a state is written, so an open detail sheet can refresh. */
    this.onStatesChanged = onStatesChanged;
    /** Called once a thumbnail has been captured, so the library can redraw the card. */
    this.onArtworkCaptured = onArtworkCaptured;
    /** Opens the cheat manager for the running game. */
    this.onOpenCheats = onOpenCheats;
    /** Opens the core's own option list. */
    this.onOpenCoreOptions = onOpenCoreOptions;
    /** Enters touch-layout edit mode. */
    this.onEditLayout = onEditLayout;

    this.root = document.getElementById('view-player');
    this.canvas = document.getElementById('gpu-canvas');
    this.chrome = document.getElementById('player-chrome');
    this.titleEl = document.getElementById('player-title');
    this.subtitleEl = document.getElementById('player-subtitle');
    this.loadingEl = document.getElementById('player-loading');
    this.loadingTitleEl = document.getElementById('loading-title');
    this.loadingDetailEl = document.getElementById('loading-detail');
    this.loadingBarEl = document.getElementById('loading-bar');
    this.unlockBtn = document.getElementById('audio-unlock');
    this.touchpad = document.getElementById('touchpad');

    this.hudEl = document.getElementById('player-hud');
    this.hudFps = document.getElementById('hud-fps');
    this.hudMem = document.getElementById('hud-mem');
    this.hudFrames = document.getElementById('hud-frames');
    this.hudAudio = document.getElementById('hud-audio');
    this.pauseBtn = document.getElementById('ctl-pause');
    this.muteBtn = document.getElementById('ctl-mute');

    /** @type {import('../data/catalog.js').CatalogEntry|null} */
    this.entry = null;
    this.paused = false;
    this.active = false;
    this._hudTick = 0;
    /** One artwork capture attempt per launch, whether it succeeds or not. */
    this._captureAttempted = false;
    this._chromeTimer = 0;
    this._launchToken = 0;

    /** Which core is driving the current session, for tagging states. */
    this._sessionCore = null;

    /** Wall-clock time of the last automatic save, for throttling. */
    this._lastAutoSave = 0;

    /**
     * Set while an auto-save is in flight so two triggers cannot race into the same
     * record — `visibilitychange` and `exit` fire together when a PWA is closed.
     */
    this._autoSaveInFlight = false;

    this.tick = this.tick.bind(this);
    this._wire();
  }

  /**
   * How often emulation is checkpointed while a game runs.
   *
   * The promise is that progress is never lost to closing the app, and the honest way
   * to keep it is not to rely on shutdown hooks: iOS can drop a backgrounded tab
   * without firing anything reliable, and an IndexedDB write started during teardown
   * may not commit. A periodic checkpoint makes the worst case bounded — at most this
   * many seconds — regardless of how the app goes away.
   *
   * Thirty seconds is a compromise: a Mega Drive state is a megabyte, and structured-
   * cloning that has a cost, so this is deliberately not every frame or every second.
   */
  static AUTO_SAVE_INTERVAL_MS = 30_000;

  _wire() {
    document.getElementById('player-back').addEventListener('click', () => this.exit());
    this.pauseBtn.addEventListener('click', () => this.togglePause());
    document.getElementById('ctl-reset').addEventListener('click', () => this.reset());
    this.muteBtn.addEventListener('click', () => this.toggleMute());
    document.getElementById('ctl-save').addEventListener('click', () => this.saveState());
    document.getElementById('ctl-cheats')?.addEventListener('click', () => {
      if (this.entry) this.onOpenCheats?.(this.entry.id);
    });
    document.getElementById('ctl-coreopts')?.addEventListener('click', () =>
      this.onOpenCoreOptions?.(),
    );
    document.getElementById('ctl-layout')?.addEventListener('click', () => {
      // The chrome is idled on the way in: it sits over the pad being arranged, and the
      // editor has its own Done button to come back through. `is-idle` is the same class
      // the auto-hide timer uses, so there is one mechanism rather than two.
      clearTimeout(this._chromeTimer);
      this.chrome.classList.add('is-idle');
      this.onEditLayout?.();
    });

    // These write the *setting*, not just the live session, so a choice made here is
    // remembered and the Settings sheet shows the same value. `syncDisplaySettings`
    // reads it back, which keeps one direction of truth.
    document.getElementById('ctl-scale').addEventListener('change', (event) => {
      setSetting('scaleMode', event.target.value);
      this.host.bridge?.setScaleMode(event.target.value);
      this.loop.wake();
    });
    document.getElementById('ctl-filter').addEventListener('change', (event) => {
      setSetting('filter', event.target.value);
      this.host.bridge?.setFilter(event.target.value);
      this.loop.wake();
    });
    document.getElementById('ctl-speed').addEventListener('change', (event) => {
      this.host.bridge?.setSpeed(Number(event.target.value));
    });

    this.unlockBtn.addEventListener('click', async () => {
      await this.audio.resume();
      this._syncAudioUi();
    });

    this.audio.onStateChange = () => this._syncAudioUi();

    // Chrome auto-hides during play and returns on any pointer movement.
    for (const type of ['pointermove', 'pointerdown']) {
      this.root.addEventListener(type, () => this._showChrome(), { passive: true });
    }

    document.addEventListener('keydown', (event) => {
      if (!this.active) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        this.exit();
      } else if (event.key === 'p' || event.key === 'P') {
        this.togglePause();
      }
    });

    // Losing visibility mid-game should pause, not fast-forward on return. The
    // Rust pacer would resync anyway, but a paused game is the expected behaviour.
    document.addEventListener('visibilitychange', () => {
      if (document.hidden && this.active && !this.paused) this.togglePause();
    });
  }

  // -------------------------------------------------------------------- launch

  /**
   * Boots a game. Rejections are reported and leave the UI back in the library.
   * @param {string} entryId
   * @param {{coreId?: string|null}} [options] `coreId` overrides the core for this
   *   launch. An id that cannot run the game's system is ignored by the registry, so
   *   passing a stale one is safe.
   */
  async launch(entryId, options = {}) {
    const entry = entryById(entryId);
    if (!entry) return;

    // An explicit choice for this launch beats the stored preference, which beats the
    // manifest default. All three go through the same registry call.
    const preferredCore = options.coreId ?? getCorePreference(entry.systemId);

    const system = getSystem(entry.systemId);
    if (system?.phase === 2) {
      toast(
        `${system.name} is Phase 2`,
        'This system needs the JIT-capable native build; the browser cannot run it.',
        { kind: 'warn' },
      );
      return;
    }

    // Start the audio graph while the launching gesture is still "live". Awaiting
    // anything before this is what leaves iOS silent.
    const audioStart = this.audio.ensureStarted();

    const token = ++this._launchToken;

    // Free the previous session's core *before* fetching or instantiating the next one.
    // Launching straight from one game into another would otherwise hold two cores in
    // memory at the same time — an NES core plus a GBA core is ~40 MB, and on iOS that
    // spike is exactly what gets a process killed.
    if (this.active || this.host.bridge?.status === 'running' || this.host.bridge?.status === 'paused') {
      this.loop.setEngineActive(false);
      this.input.setEnabled(false);
      this.host.bridge.stop();
      this.audio.flush();
      this.active = false;
    }

    this.entry = entry;
    this.paused = false;
    // Per-launch, not per-entry: relaunching a game whose capture failed (a GPU that
    // cannot map a buffer, a black title screen) is a fair reason to try once more.
    this._captureAttempted = false;

    this._show();
    this.syncHudVisibility();
    this._setLoading(true, 'Preparing engine…', 'Loading WebAssembly bridge');
    this.titleEl.textContent = entry.title;
    this.subtitleEl.textContent = system ? system.name : entry.systemId;

    try {
      await this.host.load();
      if (token !== this._launchToken) return; // Superseded by a newer launch.

      this._setLoading(true, 'Initialising WebGPU…', 'Requesting adapter and device');
      await this.host.initGpu(this.canvas);
      if (token !== this._launchToken) return;

      const coreId = this.coreLoader.coreIdFor(entry.systemId, preferredCore);
      if (!coreId) throw new Error(`no core is declared for system '${entry.systemId}'`);

      const coreEntry = this.coreLoader.entryFor(coreId);
      const resident = this.coreLoader.isResident(coreId);
      this._setLoading(
        true,
        resident ? 'Starting core…' : `Downloading ${coreEntry?.name ?? coreId}…`,
        resident ? 'Core already resident' : 'Fetching module',
      );

      await this.coreLoader.ensureCore(coreId, (progress) => {
        if (token !== this._launchToken) return;
        this._setProgress(progress);
      });
      if (token !== this._launchToken) return;

      this._setLoading(true, 'Loading content…', entry.title);
      const content = await getContent(entry);
      if (token !== this._launchToken) return;

      // The filename matters: real cores resolve their content-info overrides from
      // the extension and reject content whose extension they cannot see.
      this.host.bridge.launch(coreId, entry.id, content, contentFilename(entry));

      // Recorded now, while the core that will run this session is known. Save states
      // are tagged with it, and a state is only meaningful to the build that wrote it.
      this._sessionCore = this.coreLoader.identityFor(coreId);
      this._lastAutoSave = Date.now();

      // The saved display preferences, not whatever the dropdowns happen to show. The
      // dropdowns are then synced to match, so the player's own controls and the Settings
      // sheet can never disagree about what is in force.
      this.syncDisplaySettings();
      this.host.bridge.setSpeed(Number(document.getElementById('ctl-speed').value));
      this.host.syncCanvasSize();

      await audioStart;
      if (token !== this._launchToken) return;
      this.audio.flush();

      // Resume before the loop starts, so the first frame presented is already the
      // restored one — restoring after a few frames have run shows a flash of the
      // game's boot screen, which reads as a bug.
      this._setLoading(true, 'Restoring…', 'Reading your last checkpoint');
      const resumed = await this._resumeIfPossible(token);
      if (token !== this._launchToken) return;

      // Cheats live inside the core and die with it, so the game's stored list is
      // pushed on every launch. After the resume, because loading a state re-pushes
      // them anyway and doing it here keeps the order obvious.
      this._applyStoredCheats();
      this._applyStoredCoreOptions(coreId);

      markPlayed(entry.id);
      this._setLoading(false);
      this._startLoop();

      this.subtitleEl.textContent = coreEntry?.placeholder
        ? `${system?.name ?? entry.systemId} · placeholder core (diagnostic pattern)`
        : `${system?.name ?? entry.systemId} · ${coreEntry?.name ?? coreId}` +
          (entry.source === 'builtin' ? ' · test cart' : '') +
          (resumed ? ' · resumed' : '');
      this._syncAudioUi();
      this._showChrome();
    } catch (err) {
      this._setLoading(false);
      this.host.reportError('Launch failed', err);
      this.exit();
    }
  }

  _startLoop() {
    this.active = true;
    this.input.setEnabled(true);
    this.input.onPadsChanged = (summary) => this._syncPadIndicator(summary);
    this._syncPadIndicator({ pads: this.host.bridge?.connectedPads ?? 0, labels: [] });
    this.loop.setEngineTick(this.tick);
    this.loop.setEngineActive(true);
    this.pauseBtn.classList.remove('is-active');
  }

  // ---------------------------------------------------------------------- tick

  /**
   * One frame of the whole system. Called by the shared loop; never call directly.
   * @param {number} now rAF timestamp
   */
  tick(now) {
    // 1. Input.
    this.input.pollGamepads();

    // 2 & 4. Core steps and GPU present, both inside Rust.
    const stats = this.host.tick(now);

    // 3. Audio: hand the PCM this tick produced to the device.
    this.audio.pump();

    if (++this._hudTick >= HUD_INTERVAL) {
      this._hudTick = 0;
      this._updateHud(stats);
      // 5. Periodic checkpoint. Piggy-backed on the HUD's slow tick rather than given
      // its own timer, so it cannot fire while the loop is idle or a game is paused —
      // and `autoSave` throttles it to AUTO_SAVE_INTERVAL_MS regardless of how often
      // it is called. Not awaited: a frame must never wait on storage.
      if (this.active && !this.paused) void this.autoSave('periodic');
      // 6. Cover art, once, for a game that has none. Same reasoning: on the slow
      // tick, never awaited, and guarded so it cannot run twice.
      if (this.active && !this.paused) this._maybeCaptureArtwork(stats);
    }
  }

  /**
   * Tier 4 of the cover-art ladder: keep a thumbnail of what the game actually draws.
   *
   * Frame 60 is the threshold because the first frames of a console are not the game.
   * A cold boot shows a blank screen, then a logo, then usually a black fade — capturing
   * at frame 1 reliably produces a black square. One second in, a title screen or the
   * first room is on screen, which is a recognisable thumbnail.
   *
   * The capture costs a texture, a staging buffer and a GPU buffer map, so it happens
   * exactly once per game and only when there is no artwork already. It is also
   * completely optional: a failure is logged and forgotten, because a missing thumbnail
   * is not worth interrupting a game for. Headless and software GPU stacks often cannot
   * map a buffer at all, which is precisely such a failure.
   */
  _maybeCaptureArtwork(stats) {
    if (this._captureAttempted) return;
    if (!this.entry || this.entry.art) return;
    if (!getSetting('captureArtwork')) return;
    if (stats.frameCount < CAPTURE_AFTER_FRAME) return;

    this._captureAttempted = true;
    const entryId = this.entry.id;

    void (async () => {
      try {
        // Asked for at thumbnail size, so Rust scales the frame on the GPU and no
        // resampling happens in JavaScript.
        const rgba = await this.host.captureFrame(CAPTURE_WIDTH, CAPTURE_HEIGHT);
        // The user may have exited, or picked their own image, while this was in
        // flight — either way, do not overwrite what is there now.
        const entry = entryById(entryId);
        if (!entry || entry.art) return;
        await storeCapture(entryId, rgba, CAPTURE_WIDTH, CAPTURE_HEIGHT);
        this.onArtworkCaptured?.(entryId);
        console.info(`[artwork] captured a thumbnail for ${entryId} at frame ${stats.frameCount}`);
      } catch (err) {
        console.info('[artwork] frame capture unavailable, keeping the generated plate:', err);
      }
    })();
  }

  /**
   * Pushes this game's stored cheat list into the freshly started core.
   *
   * Never fatal. A core that does not implement the cheat entry points, or a code it
   * rejects, must not stop a game from starting — so the failure is reported once and
   * the session continues without cheats.
   */
  _applyStoredCheats() {
    if (!this.entry) return 0;
    const { codes, enabled } = cheatStore.payloadFor(this.entry.id);
    if (!codes.length) return 0;

    if (!this.host.bridge?.cheatsSupported) {
      toast(
        'Cheats not applied',
        `${this._sessionCore?.name ?? 'This core'} does not support cheats.`,
        { kind: 'warn' },
      );
      return 0;
    }
    try {
      const active = this.host.bridge.applyCheats(codes, enabled);
      console.info(`[cheats] applied ${active} of ${codes.length} for ${this.entry.id}`);
      return active;
    } catch (err) {
      console.warn('[cheats] could not apply', err);
      toast('Cheats not applied', String(err?.message ?? err), { kind: 'warn', ms: 7000 });
      return 0;
    }
  }

  /**
   * Pushes this core's stored option values into the freshly loaded core.
   *
   * Applied after content is loaded rather than before, because that is the only window
   * this code has — and it works because the core re-reads changed variables when
   * `GET_VARIABLE_UPDATE` reports a change, which `core-runtime.js` now does. Options a
   * core only consults while loading need a relaunch, and the Core options sheet says so.
   */
  _applyStoredCoreOptions(coreId) {
    const stored = coreOptionStore.valuesFor(coreId);
    const keys = Object.keys(stored);
    if (!keys.length) return 0;

    let applied = 0;
    for (const key of keys) {
      try {
        this.host.bridge.setCoreOption(key, stored[key]);
        applied++;
      } catch (err) {
        // A key this core build no longer declares. Logged, not surfaced: it is a stale
        // preference, not something the user did wrong, and it cannot break the session.
        console.info(`[core-options] skipping '${key}' for ${coreId}: ${err?.message ?? err}`);
      }
    }
    if (applied) console.info(`[core-options] applied ${applied}/${keys.length} for ${coreId}`);
    return applied;
  }

  /**
   * Applies the saved scaling and filtering to the running session, and syncs the
   * player's dropdowns to match.
   *
   * One direction of truth: the setting is authoritative and the dropdowns display it.
   * Changing a dropdown still writes the setting (wired below), so the two stay in step
   * whichever one the user touches.
   */
  syncDisplaySettings() {
    const scale = getSetting('scaleMode');
    const filter = getSetting('filter');
    const scaleEl = document.getElementById('ctl-scale');
    const filterEl = document.getElementById('ctl-filter');
    if (scaleEl) scaleEl.value = scale;
    if (filterEl) filterEl.value = filter;
    if (this.host.bridge?.status === 'running' || this.host.bridge?.status === 'paused') {
      this.host.bridge.setScaleMode(scale);
      this.host.bridge.setFilter(filter);
    }
  }

  /** Applies the performance-HUD setting. Off by default on phones. */
  syncHudVisibility() {
    const show = getSetting('showHud');
    if (this.hudEl) this.hudEl.hidden = !show;
  }

  _updateHud(stats) {
    this.hudFps.textContent = stats.displayFps ? stats.displayFps.toFixed(0) : '—';
    this.hudFrames.textContent = Math.round(stats.frameCount).toLocaleString();

    // Audio latency in ms, from the Rust ring's queue depth.
    const rate = this.audio.sampleRate || this.host.bridge?.outputSampleRate || 48000;
    const ringMs = (stats.audioQueuedFrames / rate) * 1000;
    const bufferedMs = ringMs + this.audio.bufferedMs;
    this.hudAudio.textContent = bufferedMs.toFixed(0);

    // Memory held by the running core: module plus its staging buffers. Worth showing
    // because it is the number the multi-core retention policy exists to bound.
    if (this.hudMem) {
      const bytes = this.host.bridge?.sessionCoreMemoryBytes;
      this.hudMem.textContent = bytes ? (bytes / 1024 / 1024).toFixed(1) : '—';
    }

    // Amber when the frame budget is being missed or audio is starving — the two
    // things worth noticing at a glance.
    this.hudFps.parentElement.classList.toggle('is-warn', stats.dropped > 0);
    this.hudAudio.parentElement.classList.toggle(
      'is-warn',
      this.audio.isRunning && bufferedMs < 12,
    );
  }

  // ------------------------------------------------------------------ controls

  togglePause() {
    if (!this.active || !this.host.bridge) return;
    this.paused = !this.paused;
    if (this.paused) {
      this.host.bridge.pause();
      void this.audio.suspend();
    } else {
      this.host.bridge.resume(performance.now());
      void this.audio.resume();
    }
    this.pauseBtn.classList.toggle('is-active', this.paused);
    this.pauseBtn.setAttribute('aria-label', this.paused ? 'Resume' : 'Pause');
    // Paused still ticks, to keep presenting on resize.
    this.loop.wake(4);
  }

  reset() {
    if (!this.active || !this.host.bridge) return;
    try {
      this.host.bridge.reset();
      this.audio.flush();
      toast('Core reset', this.entry?.title ?? '');
    } catch (err) {
      this.host.reportError('Reset failed', err);
    }
  }

  toggleMute() {
    const bridge = this.host.bridge;
    if (!bridge) return;
    const muted = !bridge.isMuted;
    bridge.setMuted(muted);
    this.audio.setMuted(muted);
    this.muteBtn.classList.toggle('is-active', muted);
    this.muteBtn.setAttribute('aria-label', muted ? 'Unmute' : 'Mute');
  }

  /**
   * Captures the core's state. Synchronous on purpose: everything asynchronous
   * happens afterwards, because by the time a teardown path has awaited anything the
   * core may already have been freed.
   *
   * @returns {Uint8Array|null}
   */
  _captureState() {
    if (!this.entry || !this.host.bridge) return null;
    if (this.host.bridge.status !== 'running' && this.host.bridge.status !== 'paused') {
      return null;
    }
    try {
      return this.host.bridge.saveState();
    } catch (err) {
      // A core that does not support states is not an error worth interrupting play
      // for; it just means there is nothing to checkpoint.
      console.warn('[states] core could not produce a state:', err.message ?? err);
      return null;
    }
  }

  /** Manual save, from the Save state button. */
  async saveState() {
    if (!this.active || !this.entry) return null;
    const bytes = this._captureState();
    if (!bytes) {
      toast('Nothing to save', 'This core does not support save states.', { kind: 'warn' });
      return null;
    }
    try {
      const record = await saveStates.put({
        gameId: this.entry.id,
        bytes,
        frame: this.host.stats.frameCount,
        core: this._sessionCore ?? { id: 'unknown' },
      });
      toast('State saved', `Slot ${record.slot} · ${record.sizeKb.toFixed(1)} KB · kept on device`);
      this.onStatesChanged?.(this.entry.id);
      return record;
    } catch (err) {
      this.host.reportError('Save state failed', err);
      return null;
    }
  }

  /**
   * Checkpoints the running game into its single auto-save slot.
   *
   * @param {string} why for the log, so a lost-progress report can be traced
   * @param {{force?: boolean}} [options] force skips the interval throttle
   */
  async autoSave(why, { force = false } = {}) {
    if (!this.entry) return null;
    if (this._autoSaveInFlight) return null;
    const now = Date.now();
    if (!force && now - this._lastAutoSave < PlayerView.AUTO_SAVE_INTERVAL_MS) return null;

    const bytes = this._captureState();
    if (!bytes) return null;

    // Claimed before awaiting: `exit()` and `visibilitychange` fire together when a
    // PWA closes, and two writes to one record is a corrupt record.
    this._autoSaveInFlight = true;
    this._lastAutoSave = now;
    const gameId = this.entry.id;
    try {
      const record = await saveStates.put({
        gameId,
        bytes,
        frame: this.host.stats.frameCount,
        auto: true,
        core: this._sessionCore ?? { id: 'unknown' },
      });
      console.info(`[states] auto-saved ${gameId} at frame ${record.frame} (${why})`);
      this.onStatesChanged?.(gameId);
      return record;
    } catch (err) {
      // Never interrupt play for this. A failed checkpoint is worth a log and, if
      // storage is full, one toast — not a modal in the middle of a game.
      console.warn(`[states] auto-save failed (${why}):`, err.message ?? err);
      if (String(err?.message ?? '').includes('storage is full')) {
        toast('Could not auto-save', 'Device storage is full.', { kind: 'warn' });
      }
      return null;
    } finally {
      this._autoSaveInFlight = false;
    }
  }

  /**
   * Restores the auto-save for the game that just launched, if there is one and it
   * still matches the core.
   *
   * @returns {Promise<boolean>} whether the session was resumed
   */
  async _resumeIfPossible(token) {
    if (!this.entry) return false;
    const record = saveStates.autoStateFor(this.entry.id);
    if (!record) return false;

    const verdict = saveStates.compatibility(record, {
      coreId: this._sessionCore?.id,
      coreVersion: this._sessionCore?.version,
      // The length the running core would produce now: the strongest available
      // check, and the one that catches a layout change with no version bump.
      stateSize: this._captureState()?.length,
    });
    if (!verdict.ok) {
      toast('Could not resume', saveStates.describeIncompatibility(verdict), {
        kind: 'warn',
        ms: 9000,
      });
      return false;
    }

    const bytes = await saveStates.payloadFor(this.entry.id, saveStates.AUTO_SLOT);
    if (!bytes || token !== this._launchToken) return false;
    try {
      this.host.bridge.loadState(bytes);
      this.audio.flush();
      toast('Resumed', `Picked up where you left off · frame ${record.frame.toLocaleString()}`);
      return true;
    } catch (err) {
      // The gate passed but the core still refused: report it and carry on from the
      // start rather than leaving the user on a black screen.
      console.warn('[states] resume rejected by the core:', err.message ?? err);
      toast('Could not resume', 'The core rejected the saved state; starting fresh.', {
        kind: 'warn',
      });
      return false;
    }
  }

  /**
   * Loads a stored state into the running session.
   * @param {string} gameId
   * @param {number} slot
   */
  async loadState(gameId, slot) {
    if (this.host.bridge?.currentContentId !== gameId) {
      toast('Load that game first', 'States can only be loaded into a running session.', {
        kind: 'warn',
      });
      return;
    }

    const record = saveStates.listFor(gameId).find((s) => s.slot === slot);
    const verdict = saveStates.compatibility(record, {
      coreId: this._sessionCore?.id,
      coreVersion: this._sessionCore?.version,
      stateSize: this._captureState()?.length,
    });
    if (!verdict.ok) {
      toast('State not loaded', saveStates.describeIncompatibility(verdict), {
        kind: 'warn',
        ms: 9000,
      });
      return;
    }

    const bytes = await saveStates.payloadFor(gameId, slot);
    if (!bytes) {
      toast('State not available', 'Its data is no longer stored on this device.', {
        kind: 'warn',
      });
      return;
    }
    try {
      this.host.bridge.loadState(bytes);
      this.audio.flush();
      toast('State loaded', `Slot ${slot} · frame ${record.frame.toLocaleString()}`);
    } catch (err) {
      this.host.reportError('Load state failed', err);
    }
  }

  // ---------------------------------------------------------------- visibility

  exit() {
    // Checkpoint first. `stop()` ends the session and, under the default retention
    // policy, frees the core — after that there is nothing left to serialise. The
    // capture is synchronous for exactly this reason; only the write is deferred.
    void this.autoSave('exit', { force: true });

    this._launchToken++; // Cancels any in-flight launch.
    this.active = false;
    this.paused = false;
    this.input.setEnabled(false);
    this.input.releaseTouch();
    this.loop.setEngineActive(false);
    this.host.bridge?.stop();
    this._sessionCore = null;
    this.host.resetStats();
    this.audio.flush();
    void this.audio.suspend();
    this._setLoading(false);
    this.root.hidden = true;
    this.touchpad.hidden = true;
    document.getElementById('app').dataset.view = 'library';
    this.entry = null;
    this.onExit();
  }

  _show() {
    document.getElementById('app').dataset.view = 'player';
    this.root.hidden = false;
    this.touchpad.hidden = false;
  }

  _showChrome() {
    this.chrome.classList.remove('is-idle');
    clearTimeout(this._chromeTimer);
    if (!this.active) return;
    this._chromeTimer = setTimeout(() => {
      if (this.active && !this.paused) this.chrome.classList.add('is-idle');
    }, CHROME_IDLE_MS);
  }

  /** Shows how input is reaching the core: keyboard glyph, or the pad count. */
  _syncPadIndicator({ pads, labels }) {
    const el = document.getElementById('hud-pads');
    if (!el) return;
    if (pads > 0) {
      el.textContent = pads === 1 ? '🎮' : `🎮×${pads}`;
      el.title = labels.length ? labels.join(', ') : `${pads} controller(s) connected`;
    } else {
      el.textContent = '⌨';
      el.title = 'Keyboard: arrows, Z/X, Enter, Shift';
    }
  }

  _syncAudioUi() {
    const needsGesture = this.active && !this.audio.isRunning && this.audio.state !== 'unsupported';
    this.unlockBtn.hidden = !needsGesture;
    if (this.audio.state === 'unsupported' && this.active) {
      this.hudAudio.textContent = 'n/a';
    }
  }

  _setLoading(visible, title = '', detail = '') {
    this.loadingEl.hidden = !visible;
    if (visible) {
      this.loadingTitleEl.textContent = title;
      this.loadingDetailEl.textContent = detail;
      if (!title.startsWith('Downloading')) this.loadingBarEl.style.width = '0%';
    }
  }

  _setProgress({ received, total, phase }) {
    if (phase === 'cache') {
      this.loadingDetailEl.textContent = 'Loaded from cache';
      this.loadingBarEl.style.width = '100%';
      return;
    }
    if (phase === 'instantiate') {
      this.loadingDetailEl.textContent = 'Instantiating core';
      this.loadingBarEl.style.width = '100%';
      return;
    }
    if (total > 0) {
      const pct = Math.min(100, Math.round((received / total) * 100));
      this.loadingBarEl.style.width = `${pct}%`;
      this.loadingDetailEl.textContent = `${(received / 1024).toFixed(0)} KB of ${(total / 1024).toFixed(0)} KB`;
    } else {
      this.loadingDetailEl.textContent = `${(received / 1024).toFixed(0)} KB`;
    }
  }
}
