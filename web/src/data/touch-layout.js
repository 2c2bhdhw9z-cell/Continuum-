/**
 * The on-screen pad's layout: where the controls sit, how big they are, how solid.
 *
 * ## Why CSS custom properties
 *
 * The layout is expressed as six numbers written to custom properties on the touchpad
 * element, and the stylesheet does the arithmetic. That means dragging a control is one
 * property write per frame — no reflow of a class list, no inline `left/top` per button,
 * and no JS in the layout path at all once the value is set. It also means the same
 * numbers describe the layout on a phone in portrait and a tablet in landscape, because
 * they are proportions of the safe area rather than pixels.
 *
 * ```text
 *   --tp-scale      0.7 … 1.6    size multiplier for every control
 *   --tp-opacity    0.15 … 1     resting opacity
 *   --tp-dpad-x/y   0 … 1        D-pad centre, as a fraction of the play area
 *   --tp-face-x/y   0 … 1        face-button cluster centre
 * ```
 *
 * ## Why IndexedDB and not localStorage
 *
 * Unlike the other preferences here, a layout is not needed synchronously at boot — the
 * touchpad is only shown once a game starts, which is already asynchronous — and it is
 * the one preference a user might plausibly want to survive a storage purge that clears
 * cookies-and-site-data-except-databases. It also keeps the localStorage payload from
 * growing with per-device variants.
 */

import { openDatabase, runTransaction, requestToPromise, SETTINGS } from './idb.js';

/** Bounds, so a layout restored from storage can never put a control off-screen. */
export const LIMITS = {
  scale: { min: 0.7, max: 1.6 },
  opacity: { min: 0.15, max: 1 },
  // Kept off the very edge: a control centred at 0 would be half outside the viewport,
  // and on iOS the outer few millimetres belong to the system's edge gestures.
  x: { min: 0.08, max: 0.92 },
  y: { min: 0.12, max: 0.9 },
};

/**
 * The default layout: thumbs rest at the bottom corners.
 *
 * Not centred vertically, because a phone held in landscape is gripped at the bottom
 * corners and the middle of the screen is where the game is.
 */
export function defaults() {
  return {
    scale: 1,
    opacity: 0.55,
    dpadX: 0.16,
    dpadY: 0.66,
    faceX: 0.84,
    faceY: 0.66,
  };
}

const KEY = 'touch-layout';

function clamp(value, { min, max }, fallback) {
  const number = Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.min(max, Math.max(min, number));
}

/** Forces any stored or user-supplied layout into range. */
export function sanitise(layout) {
  const base = defaults();
  if (!layout || typeof layout !== 'object') return base;
  return {
    scale: clamp(layout.scale, LIMITS.scale, base.scale),
    opacity: clamp(layout.opacity, LIMITS.opacity, base.opacity),
    dpadX: clamp(layout.dpadX, LIMITS.x, base.dpadX),
    dpadY: clamp(layout.dpadY, LIMITS.y, base.dpadY),
    faceX: clamp(layout.faceX, LIMITS.x, base.faceX),
    faceY: clamp(layout.faceY, LIMITS.y, base.faceY),
  };
}

/** @type {object|null} */
let cached = null;

/** The current layout, synchronously. Defaults until `load()` has resolved. */
export function current() {
  return cached ?? defaults();
}

export async function load() {
  try {
    const db = await openDatabase();
    const row = await requestToPromise(
      db.transaction(SETTINGS, 'readonly').objectStore(SETTINGS).get(KEY),
    );
    cached = sanitise(row?.value);
  } catch (err) {
    console.warn('[touch] could not read the saved layout', err);
    cached = defaults();
  }
  return cached;
}

export async function save(layout) {
  cached = sanitise(layout);
  try {
    const db = await openDatabase();
    await runTransaction(db, [SETTINGS], 'readwrite', (transaction) => {
      transaction.objectStore(SETTINGS).put({ key: KEY, value: cached, updatedAt: Date.now() });
    });
  } catch (err) {
    console.warn('[touch] could not save the layout', err);
  }
  return cached;
}

export async function reset() {
  return save(defaults());
}

/**
 * Writes a layout onto an element as custom properties.
 *
 * Everything else — the size of each button, where the cluster's members sit relative to
 * its centre, how opacity changes while a button is held — is in `player.css`. This
 * function's whole job is six numbers.
 *
 * @param {HTMLElement} element
 * @param {object} layout
 */
export function apply(element, layout = current()) {
  const l = sanitise(layout);
  element.style.setProperty('--tp-scale', String(l.scale));
  element.style.setProperty('--tp-opacity', String(l.opacity));
  element.style.setProperty('--tp-dpad-x', `${(l.dpadX * 100).toFixed(2)}%`);
  element.style.setProperty('--tp-dpad-y', `${(l.dpadY * 100).toFixed(2)}%`);
  element.style.setProperty('--tp-face-x', `${(l.faceX * 100).toFixed(2)}%`);
  element.style.setProperty('--tp-face-y', `${(l.faceY * 100).toFixed(2)}%`);
}
