/**
 * Edit mode for the on-screen pad: drag the two clusters, and scale/fade the whole thing.
 *
 * ## Pointer Events, not touch events
 *
 * One code path handles a finger, a stylus and a mouse, and `setPointerCapture` means a
 * drag that leaves the element still tracks — which matters because the thing being
 * dragged is being dragged *by the thing that would otherwise stop receiving events*.
 * Touch events would need a parallel mouse implementation for anyone customising the
 * layout on a desktop before picking up their phone.
 *
 * ## Why the buttons stop working while editing
 *
 * In edit mode `.tp { pointer-events: none }`, so a drag that starts on the D-pad's Up
 * button is received by the cluster rather than by the button. Without that, moving the
 * pad would fire the input it is made of, and the game would take a fistful of button
 * presses while being rearranged.
 *
 * The layout is written to CSS custom properties on every pointer move and saved to
 * IndexedDB once, on release — a database write per frame of a drag would be absurd.
 */

import * as layoutStore from '../data/touch-layout.js';
import { toast } from './toast.js';

export class TouchEditor {
  /**
   * @param {object} opts
   * @param {() => void} [opts.onChanged]
   */
  constructor({ onChanged } = {}) {
    this.onChanged = onChanged ?? (() => {});

    this.pad = document.getElementById('touchpad');
    this.panel = document.getElementById('touch-editor');
    this.scaleInput = document.getElementById('touch-scale');
    this.opacityInput = document.getElementById('touch-opacity');
    this.scaleValueEl = document.getElementById('touch-scale-value');
    this.opacityValueEl = document.getElementById('touch-opacity-value');
    this.doneBtn = document.getElementById('touch-done');
    this.resetBtn = document.getElementById('touch-reset');

    this.editing = false;
    /** Working copy; committed to storage on release or on Done. */
    this.layout = layoutStore.current();

    this._wire();
  }

  /** Reads the saved layout and applies it. Called once at boot. */
  async init() {
    this.layout = await layoutStore.load();
    this.apply();
    return this.layout;
  }

  apply() {
    if (this.pad) layoutStore.apply(this.pad, this.layout);
    this._syncInputs();
  }

  _syncInputs() {
    if (this.scaleInput) this.scaleInput.value = String(this.layout.scale);
    if (this.opacityInput) this.opacityInput.value = String(this.layout.opacity);
    if (this.scaleValueEl) this.scaleValueEl.textContent = `${Math.round(this.layout.scale * 100)}%`;
    if (this.opacityValueEl) {
      this.opacityValueEl.textContent = `${Math.round(this.layout.opacity * 100)}%`;
    }
  }

  _wire() {
    if (!this.pad) return;

    for (const cluster of this.pad.querySelectorAll('.touchpad__dpad, .touchpad__face')) {
      cluster.addEventListener('pointerdown', (event) => this._startDrag(event, cluster));
    }

    // `input`, not `change`: a slider should show its effect while being dragged.
    this.scaleInput?.addEventListener('input', () => {
      this.layout = { ...this.layout, scale: Number(this.scaleInput.value) };
      this.apply();
    });
    this.opacityInput?.addEventListener('input', () => {
      this.layout = { ...this.layout, opacity: Number(this.opacityInput.value) };
      this.apply();
    });
    // Saved on release rather than on every input event, so dragging a slider is not
    // dozens of database writes.
    for (const input of [this.scaleInput, this.opacityInput]) {
      input?.addEventListener('change', () => void this._commit());
    }

    this.doneBtn?.addEventListener('click', () => void this.stop());
    this.resetBtn?.addEventListener('click', () => void this._reset());
  }

  // ------------------------------------------------------------------ dragging

  _startDrag(event, cluster) {
    if (!this.editing) return;
    event.preventDefault();
    const isDpad = cluster.classList.contains('touchpad__dpad');
    const box = this.pad.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) return;

    cluster.classList.add('is-dragging');
    // Capture, so the drag survives the pointer leaving the element it started on.
    cluster.setPointerCapture(event.pointerId);

    const move = (moveEvent) => {
      // Fractions of the pad, not pixels: the same layout then describes any viewport.
      const x = (moveEvent.clientX - box.left) / box.width;
      const y = (moveEvent.clientY - box.top) / box.height;
      this.layout = layoutStore.sanitise({
        ...this.layout,
        ...(isDpad ? { dpadX: x, dpadY: y } : { faceX: x, faceY: y }),
      });
      // Property write only — no reflow of anything but this cluster's transform.
      layoutStore.apply(this.pad, this.layout);
    };

    const end = () => {
      cluster.classList.remove('is-dragging');
      cluster.removeEventListener('pointermove', move);
      cluster.removeEventListener('pointerup', end);
      cluster.removeEventListener('pointercancel', end);
      void this._commit();
    };

    cluster.addEventListener('pointermove', move);
    cluster.addEventListener('pointerup', end);
    cluster.addEventListener('pointercancel', end);
  }

  async _commit() {
    this.layout = await layoutStore.save(this.layout);
    this._syncInputs();
    this.onChanged();
  }

  async _reset() {
    this.layout = await layoutStore.reset();
    this.apply();
    toast('Controls reset', 'Back to the default position, size and opacity.');
  }

  // ------------------------------------------------------------- mode toggling

  start() {
    if (!this.pad) return;
    this.editing = true;
    this.pad.classList.add('is-editing');
    // Shown even on a device with no touch, so the layout can be prepared on a desktop.
    this.pad.hidden = false;
    if (this.panel) this.panel.hidden = false;
    this._syncInputs();
  }

  async stop() {
    if (!this.pad) return;
    this.editing = false;
    this.pad.classList.remove('is-editing');
    if (this.panel) this.panel.hidden = true;
    await this._commit();
  }

  toggle() {
    if (this.editing) void this.stop();
    else this.start();
    return this.editing;
  }
}
