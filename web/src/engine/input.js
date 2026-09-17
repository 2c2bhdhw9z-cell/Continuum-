/**
 * Input plumbing: keyboard, gamepad and on-screen pad → the bridge.
 *
 * This module translates events into `setButton(port, buttonId, pressed)` and
 * nothing else. It holds no state the core cares about — latching, per-frame
 * snapshots and port routing all live in Rust, so Phase 2's Swift front end
 * reproduces the behaviour by making the same calls rather than reimplementing the
 * logic.
 *
 * Gamepads are *polled*, not evented (the Gamepad API has no button events), and
 * that poll happens inside the shared frame loop's engine tick — the "Input" stage
 * of input → core → audio → GPU. No separate timer.
 */

/** `RETRO_DEVICE_ID_JOYPAD_*`. Mirrors `input::Button` in Rust. */
export const BUTTON = {
  B: 0,
  Y: 1,
  SELECT: 2,
  START: 3,
  UP: 4,
  DOWN: 5,
  LEFT: 6,
  RIGHT: 7,
  A: 8,
  X: 9,
  L: 10,
  R: 11,
  L2: 12,
  R2: 13,
  L3: 14,
  R3: 15,
};

/**
 * Default keyboard layout. Z/X as B/A is the convention every browser emulator
 * uses, so it is what people will try first.
 */
const KEY_MAP = new Map(
  Object.entries({
    ArrowUp: BUTTON.UP,
    ArrowDown: BUTTON.DOWN,
    ArrowLeft: BUTTON.LEFT,
    ArrowRight: BUTTON.RIGHT,
    KeyZ: BUTTON.B,
    KeyX: BUTTON.A,
    KeyA: BUTTON.Y,
    KeyS: BUTTON.X,
    KeyQ: BUTTON.L,
    KeyW: BUTTON.R,
    Enter: BUTTON.START,
    ShiftRight: BUTTON.SELECT,
    ShiftLeft: BUTTON.SELECT,
    Backspace: BUTTON.SELECT,
  }),
);

/**
 * Standard-gamepad button index → retropad id.
 * Note the deliberate swap: the physical bottom face button (index 0) maps to
 * retro **B**, matching how a SNES pad's B sits under A.
 */
const PAD_MAP = new Map(
  Object.entries({
    0: BUTTON.B,
    1: BUTTON.A,
    2: BUTTON.Y,
    3: BUTTON.X,
    4: BUTTON.L,
    5: BUTTON.R,
    6: BUTTON.L2,
    7: BUTTON.R2,
    8: BUTTON.SELECT,
    9: BUTTON.START,
    10: BUTTON.L3,
    11: BUTTON.R3,
    12: BUTTON.UP,
    13: BUTTON.DOWN,
    14: BUTTON.LEFT,
    15: BUTTON.RIGHT,
  }).map(([k, v]) => [Number(k), v]),
);

/** Below this, a stick is treated as centred. Cheap drift rejection. */
const AXIS_DEADZONE = 0.18;

export class InputManager {
  /** @param {import('./bridge-host.js').BridgeHost} host */
  constructor(host) {
    this.host = host;
    this.enabled = false;
    /** Per-pad memo of last sent button states, to avoid redundant FFI calls. */
    this._padState = new Map();
    this._touchTargets = new Map();
  }

  get bridge() {
    return this.host.bridge;
  }

  /** Installs global listeners. Safe to call once at boot. */
  attach() {
    window.addEventListener('keydown', (event) => this._onKey(event, true), { passive: false });
    window.addEventListener('keyup', (event) => this._onKey(event, false), { passive: false });

    // A held key with the tab in the background would otherwise stay held forever.
    window.addEventListener('blur', () => this.releaseAll());
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) this.releaseAll();
    });

    this._attachTouchPad();
  }

  /** Input is forwarded only while a session is running. */
  setEnabled(enabled) {
    this.enabled = enabled;
    if (!enabled) this.releaseAll();
  }

  releaseAll() {
    this.bridge?.releaseAllInput();
    for (const el of this._touchTargets.keys()) el.classList.remove('is-pressed');
    this._padState.clear();
  }

  _onKey(event, pressed) {
    if (!this.enabled || !this.bridge) return;
    if (event.repeat) return;
    // Let the user keep browser shortcuts.
    if (event.metaKey || event.ctrlKey || event.altKey) return;

    const button = KEY_MAP.get(event.code);
    if (button === undefined) return;

    // Arrows and Space would scroll the page underneath the canvas.
    event.preventDefault();
    this.bridge.setButton(0, button, pressed);
  }

  _attachTouchPad() {
    const pad = document.getElementById('touchpad');
    if (!pad) return;

    for (const el of pad.querySelectorAll('.tp[data-button]')) {
      const button = Number(el.dataset.button);
      this._touchTargets.set(el, button);

      const press = (event) => {
        if (!this.enabled || !this.bridge) return;
        event.preventDefault();
        // Capture so sliding off the button still delivers the release.
        el.setPointerCapture?.(event.pointerId);
        el.classList.add('is-pressed');
        this.bridge.setButton(0, button, true);
      };
      const release = (event) => {
        if (!this.bridge) return;
        event.preventDefault();
        el.classList.remove('is-pressed');
        this.bridge.setButton(0, button, false);
      };

      el.addEventListener('pointerdown', press);
      el.addEventListener('pointerup', release);
      el.addEventListener('pointercancel', release);
    }
  }

  /**
   * Samples connected gamepads. Called from the engine tick, before the core step,
   * so a press made this frame is visible to this frame's emulation.
   */
  pollGamepads() {
    if (!this.enabled || !this.bridge) return;
    const pads = navigator.getGamepads?.();
    if (!pads) return;

    for (let padIndex = 0; padIndex < pads.length; padIndex++) {
      const pad = pads[padIndex];
      if (!pad || !pad.connected) continue;
      // Ports beyond the bridge's four are ignored rather than wrapped.
      const port = Math.min(padIndex, 3);

      let memo = this._padState.get(pad.index);
      if (!memo) {
        memo = { buttons: new Uint8Array(20), axes: new Float32Array(4) };
        this._padState.set(pad.index, memo);
      }

      for (let b = 0; b < pad.buttons.length && b < memo.buttons.length; b++) {
        const mapped = PAD_MAP.get(b);
        if (mapped === undefined) continue;
        const pressed = pad.buttons[b].pressed ? 1 : 0;
        if (memo.buttons[b] !== pressed) {
          memo.buttons[b] = pressed;
          this.bridge.setButton(port, mapped, pressed === 1);
        }
      }

      for (let a = 0; a < 4 && a < pad.axes.length; a++) {
        const raw = pad.axes[a];
        const value = Math.abs(raw) < AXIS_DEADZONE ? 0 : raw;
        // Only forward meaningful movement: sticks jitter constantly.
        if (Math.abs(value - memo.axes[a]) > 0.02) {
          memo.axes[a] = value;
          this.bridge.setAxis(port, a, value);
        }
      }
    }
  }
}
