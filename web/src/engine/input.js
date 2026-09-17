/**
 * Input plumbing: keyboard, gamepad and on-screen pad → the Rust `GamepadBridge`.
 *
 * This module forwards events and nothing more. It does not decide what a button
 * *means*: the W3C standard-gamepad layout, the deadzone, the stick-to-D-pad
 * synthesis, the merge across sources — all of that lives in Rust, so Phase 2's
 * `GameController` code reaches the same behaviour by calling the same functions.
 *
 * ## Sources are separate on purpose
 *
 * Every call names its source (`keyboard`, `touch`, `gamepad`). The Gamepad API has no
 * events, so a connected pad is polled every frame and each poll states the complete
 * condition of that pad — including "D-pad released". Merged into one layer, that
 * would cancel a keyboard press 60 times a second, and the keyboard would appear to
 * break whenever a controller was plugged in.
 *
 * ## Gamepads are polled inside the engine tick
 *
 * Not on a timer of their own. `pollGamepads()` runs as the Input stage of
 * input → core → audio → GPU, so a press made this frame is visible to the emulation
 * this frame, and there is still exactly one loop in the application.
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
 * Default keyboard layout. Z/X as B/A is what every browser emulator uses, so it is
 * what people will try first.
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

/** Buttons whose default browser action must be suppressed while playing. */
const SWALLOW_DEFAULT = new Set([
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Space',
  'Backspace',
  'Enter',
]);

export class InputManager {
  /** @param {import('./bridge-host.js').BridgeHost} host */
  constructor(host) {
    this.host = host;
    this.enabled = false;

    /** Gamepad index → assigned port. */
    this.padPorts = new Map();
    /**
     * Reused scratch buffers for the per-frame poll. Allocating a `Uint8Array` per pad
     * per frame would be 60 allocations a second for no reason.
     * @type {Map<number, {buttons: Uint8Array, axes: Float32Array}>}
     */
    this.padScratch = new Map();

    this.touchTargets = new Map();
    /** @type {(summary: {pads: number, labels: string[]}) => void} */
    this.onPadsChanged = () => {};
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

    // The Gamepad API only gives us these two events; button state must be polled.
    window.addEventListener('gamepadconnected', (event) => this._onPadConnected(event.gamepad));
    window.addEventListener('gamepaddisconnected', (event) =>
      this._onPadDisconnected(event.gamepad),
    );

    this._attachTouchPad();

    // Pads already connected before load do not fire `gamepadconnected`.
    for (const pad of navigator.getGamepads?.() ?? []) {
      if (pad) this._onPadConnected(pad);
    }
  }

  /** Input is forwarded only while a session is running. */
  setEnabled(enabled) {
    this.enabled = enabled;
    if (!enabled) this.releaseAll();
  }

  releaseAll() {
    this.bridge?.releaseAllInput();
    for (const el of this.touchTargets.keys()) el.classList.remove('is-pressed');
  }

  // ------------------------------------------------------------------ keyboard

  _onKey(event, pressed) {
    if (!this.enabled || !this.bridge) return;
    if (event.repeat) return;
    // Leave browser shortcuts alone.
    if (event.metaKey || event.ctrlKey || event.altKey) return;

    const button = KEY_MAP.get(event.code);
    if (button === undefined) return;

    if (SWALLOW_DEFAULT.has(event.code)) event.preventDefault();
    this.bridge.setButton(0, button, pressed, 'keyboard');
  }

  // ------------------------------------------------------------------ gamepads

  _onPadConnected(pad) {
    if (!this.bridge) return;
    if (this.padPorts.has(pad.index)) return;

    const port = this.bridge.firstFreePadPort() ?? 0;
    this.padPorts.set(pad.index, port);
    // "standard" is the W3C mapping; anything else gets index pass-through, which is
    // honest rather than silently wrong.
    const kind = pad.mapping === 'standard' ? 'gamepad' : 'gamepad-unmapped';
    this.bridge.connectPad(port, kind, pad.id ?? `Gamepad ${pad.index}`);
    console.info(
      `[input] gamepad ${pad.index} → port ${port}: ${pad.id} (mapping: ${pad.mapping || 'none'})`,
    );
    this._notifyPads();
  }

  _onPadDisconnected(pad) {
    const port = this.padPorts.get(pad.index);
    if (port === undefined) return;
    this.padPorts.delete(pad.index);
    this.padScratch.delete(pad.index);
    this.bridge?.disconnectPad(port);
    this._notifyPads();
  }

  _notifyPads() {
    if (!this.bridge) return;
    const labels = [];
    for (const port of this.padPorts.values()) {
      const label = this.bridge.padLabel(port);
      if (label) labels.push(label);
    }
    this.onPadsChanged({ pads: this.padPorts.size, labels });
  }

  /**
   * Samples every connected gamepad. Called from the engine tick, before the core
   * step, so a press lands in the frame it was made.
   */
  pollGamepads() {
    if (!this.enabled || !this.bridge) return;
    const pads = navigator.getGamepads?.();
    if (!pads) return;

    for (const pad of pads) {
      if (!pad || !pad.connected) continue;

      let port = this.padPorts.get(pad.index);
      if (port === undefined) {
        // A pad can appear without an event if it was connected while the tab was
        // hidden, or if the browser needed a button press to reveal it.
        this._onPadConnected(pad);
        port = this.padPorts.get(pad.index) ?? 0;
      }

      let scratch = this.padScratch.get(pad.index);
      if (!scratch || scratch.buttons.length !== pad.buttons.length) {
        scratch = {
          buttons: new Uint8Array(pad.buttons.length),
          axes: new Float32Array(Math.max(4, pad.axes.length)),
        };
        this.padScratch.set(pad.index, scratch);
      }

      for (let i = 0; i < pad.buttons.length; i++) {
        // Analog triggers report `value`; treat anything past half-travel as pressed.
        const button = pad.buttons[i];
        scratch.buttons[i] = button.pressed || button.value > 0.5 ? 1 : 0;
      }
      for (let i = 0; i < scratch.axes.length; i++) {
        scratch.axes[i] = pad.axes[i] ?? 0;
      }

      // One call per pad per frame; Rust does the mapping and the merge.
      this.bridge.applyGamepad(port, scratch.buttons, scratch.axes);
    }
  }

  // --------------------------------------------------------------- touch pad

  _attachTouchPad() {
    const pad = document.getElementById('touchpad');
    if (!pad) return;

    // The four D-pad directions are handled by the surface tracker below, not as
    // individual buttons — otherwise both handlers would fight over the same press
    // and diagonals would flicker.
    const individual = pad.querySelectorAll(
      '.tp[data-button]:not(.tp--up):not(.tp--down):not(.tp--left):not(.tp--right)',
    );
    for (const el of individual) {
      const button = Number(el.dataset.button);
      this.touchTargets.set(el, button);

      const press = (event) => {
        if (!this.enabled || !this.bridge) return;
        event.preventDefault();
        // Capture so sliding off the button still delivers its release.
        el.setPointerCapture?.(event.pointerId);
        el.classList.add('is-pressed');
        this.bridge.setButton(0, button, true, 'touch');
        // Haptics where available; ignored elsewhere.
        navigator.vibrate?.(8);
      };
      const release = (event) => {
        if (!this.bridge) return;
        event.preventDefault();
        el.classList.remove('is-pressed');
        this.bridge.setButton(0, button, false, 'touch');
      };

      el.addEventListener('pointerdown', press);
      el.addEventListener('pointerup', release);
      el.addEventListener('pointercancel', release);
      // Losing capture (a system gesture, a call) must not leave the button stuck.
      el.addEventListener('lostpointercapture', release);
    }

    // Diagonals: dragging across the D-pad should hit two directions at once, which a
    // per-button pointerdown cannot express. Tracking pointer position over the pad
    // area gives real diagonal movement.
    const dpad = pad.querySelector('.touchpad__dpad');
    if (dpad) this._attachDpadSurface(dpad);
  }

  /**
   * Treats the D-pad as one surface rather than four buttons, so a thumb between Up
   * and Right presses both. Without this, diagonal movement is impossible on touch —
   * which makes most action games unplayable.
   */
  _attachDpadSurface(dpad) {
    const directions = [
      ['.tp--up', BUTTON.UP],
      ['.tp--down', BUTTON.DOWN],
      ['.tp--left', BUTTON.LEFT],
      ['.tp--right', BUTTON.RIGHT],
    ].map(([selector, button]) => ({ el: dpad.querySelector(selector), button }));

    /** Deadzone as a fraction of the pad's radius; below it, nothing is pressed. */
    const DEADZONE = 0.22;
    /** How far off-axis a press still counts as including that direction. */
    const DIAGONAL_RATIO = 0.42;

    const apply = (event) => {
      if (!this.enabled || !this.bridge) return;
      const rect = dpad.getBoundingClientRect();
      const x = (event.clientX - (rect.left + rect.width / 2)) / (rect.width / 2);
      const y = (event.clientY - (rect.top + rect.height / 2)) / (rect.height / 2);
      const magnitude = Math.hypot(x, y);

      let up = false;
      let down = false;
      let left = false;
      let right = false;
      if (magnitude > DEADZONE) {
        // A direction counts when its component dominates, or when the other
        // component is large enough to make the press a genuine diagonal.
        if (y < 0 && Math.abs(y) > Math.abs(x) * DIAGONAL_RATIO) up = true;
        if (y > 0 && Math.abs(y) > Math.abs(x) * DIAGONAL_RATIO) down = true;
        if (x < 0 && Math.abs(x) > Math.abs(y) * DIAGONAL_RATIO) left = true;
        if (x > 0 && Math.abs(x) > Math.abs(y) * DIAGONAL_RATIO) right = true;
      }

      const pressed = { [BUTTON.UP]: up, [BUTTON.DOWN]: down, [BUTTON.LEFT]: left, [BUTTON.RIGHT]: right };
      for (const { el, button } of directions) {
        const isPressed = pressed[button];
        this.bridge.setButton(0, button, isPressed, 'touch');
        el?.classList.toggle('is-pressed', isPressed);
      }
    };

    const clear = () => {
      if (!this.bridge) return;
      for (const { el, button } of directions) {
        this.bridge.setButton(0, button, false, 'touch');
        el?.classList.remove('is-pressed');
      }
    };

    dpad.addEventListener('pointerdown', (event) => {
      event.preventDefault();
      dpad.setPointerCapture?.(event.pointerId);
      apply(event);
    });
    dpad.addEventListener('pointermove', (event) => {
      if (event.buttons === 0 && event.pointerType === 'mouse') return;
      event.preventDefault();
      apply(event);
    });
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) {
      dpad.addEventListener(type, (event) => {
        event.preventDefault();
        clear();
      });
    }
  }

  /** Releases the touch layer, e.g. when the overlay is hidden. */
  releaseTouch() {
    this.bridge?.releaseInputSource('touch');
    for (const el of this.touchTargets.keys()) el.classList.remove('is-pressed');
  }
}
