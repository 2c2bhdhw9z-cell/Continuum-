/**
 * User settings.
 *
 * localStorage rather than IndexedDB, for the same reason as `core-prefs.js`: these
 * are a handful of booleans that the UI needs *synchronously* while it is building
 * itself. Waiting on a database transaction to decide whether to show the HUD would
 * mean rendering it and then hiding it, which is a visible flicker on every launch.
 *
 * Every read falls back to an in-memory copy, because localStorage throws rather than
 * degrades in Safari private browsing and under enterprise policy. A settings screen
 * that takes the app down with it would be worse than one that forgets.
 */

const STORAGE_KEY = 'continuum:settings:v1';

/**
 * The performance HUD default is resolved from the viewport, not hard-coded.
 *
 * On a phone the HUD sits over the top of the game in the small amount of screen a
 * portrait device has, and the numbers it reports are a developer's concern. The
 * breakpoint is the same 768px the stylesheet uses for the mobile layout, so the two
 * cannot disagree about what "mobile" means.
 */
function hudDefault() {
  try {
    return !globalThis.matchMedia?.('(max-width: 768px)').matches;
  } catch {
    return true;
  }
}

function defaults() {
  return {
    /** Performance HUD over the player. Off on phones. */
    showHud: hudDefault(),
    /**
     * Whether to ask the libretro thumbnail server for cover art. On, because the
     * feature is the point — but it sends a ROM's filename to a third party, so it
     * has to be refusable and the Settings sheet says so plainly.
     */
    fetchBoxart: true,
    /** Capture a thumbnail from the game itself when no cover art was found. */
    captureArtwork: true,
  };
}

/** @type {object|null} */
let cache = null;
let memoryOnly = false;

function readAll() {
  if (cache) return cache;
  const base = defaults();
  if (memoryOnly) {
    cache = base;
    return cache;
  }
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = raw ? JSON.parse(raw) : null;
    // Unknown keys are dropped and missing ones take their default, so a settings
    // file written by an older build never leaves a value undefined.
    cache =
      parsed && typeof parsed === 'object' && !Array.isArray(parsed)
        ? { ...base, ...pickKnown(parsed, base) }
        : base;
  } catch (err) {
    console.warn('[settings] falling back to in-memory settings:', err.message);
    memoryOnly = true;
    cache = base;
  }
  return cache;
}

function pickKnown(parsed, base) {
  const out = {};
  for (const key of Object.keys(base)) {
    if (key in parsed && typeof parsed[key] === typeof base[key]) out[key] = parsed[key];
  }
  return out;
}

/** @type {Set<(settings: object) => void>} */
const listeners = new Set();

/** Subscribes to changes. Returns an unsubscribe function. */
export function onSettingsChanged(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getSettings() {
  return { ...readAll() };
}

export function getSetting(key) {
  return readAll()[key];
}

/**
 * Writes one setting and notifies listeners.
 * @param {string} key
 * @param {boolean} value
 */
export function setSetting(key, value) {
  const current = readAll();
  if (!(key in defaults())) throw new Error(`unknown setting '${key}'`);
  if (current[key] === value) return value;
  cache = { ...current, [key]: value };
  if (!memoryOnly) {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(cache));
    } catch (err) {
      console.warn('[settings] could not persist settings:', err.message);
      memoryOnly = true;
    }
  }
  const snapshot = getSettings();
  for (const listener of listeners) {
    try {
      listener(snapshot);
    } catch (err) {
      console.warn('[settings] listener threw', err);
    }
  }
  return value;
}

/** Restores defaults, including the viewport-derived HUD default. */
export function resetSettings() {
  cache = defaults();
  if (!memoryOnly) {
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      // Nothing to do: the in-memory copy is already the default.
    }
  }
  const snapshot = getSettings();
  for (const listener of listeners) listener(snapshot);
  return snapshot;
}
