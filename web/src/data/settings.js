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

    /**
     * How the emulated image fills the canvas. Passed straight to the Rust renderer's
     * existing scale modes, so this setting is a persisted default for what the player's
     * own dropdown already controlled per session.
     *
     * `'aspect'`  — fit, preserving the core's declared aspect ratio
     * `'integer'` — largest whole-pixel multiple that fits, for sharp scaling
     * `'stretch'` — fill, ignoring aspect
     */
    scaleMode: 'aspect',

    /** `'nearest'` keeps pixels crisp; `'linear'` smooths them. */
    filter: 'nearest',

    /**
     * Visual theme. Implemented as a `data-theme` attribute on `<html>` that remaps the
     * token layer, so a theme is a handful of custom-property overrides rather than a
     * parallel stylesheet.
     */
    theme: 'midnight',
  };
}

/** Themes the token layer defines. Kept here so the UI cannot offer a missing one. */
export const THEMES = [
  { id: 'midnight', name: 'Midnight', note: 'The default: near-black with a red accent.' },
  { id: 'graphite', name: 'Graphite', note: 'Neutral greys, cooler highlights.' },
  { id: 'crt', name: 'CRT Green', note: 'Phosphor green on very dark grey.' },
  { id: 'paper', name: 'Paper', note: 'Light theme, for bright rooms.' },
];

const SCALE_MODES = ['aspect', 'integer', 'stretch'];
const FILTERS = ['nearest', 'linear'];

/**
 * Applies the theme to the document.
 *
 * Separate from `setSetting` so it can also run at boot, before any listener exists, and
 * so the attribute is the single source of truth the stylesheet reads.
 */
export function applyTheme(theme = getSetting('theme')) {
  const id = THEMES.some((entry) => entry.id === theme) ? theme : 'midnight';
  document.documentElement.dataset.theme = id;
  return id;
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

/** Values an enum-valued setting is allowed to take. */
const ALLOWED = {
  scaleMode: SCALE_MODES,
  filter: FILTERS,
  theme: null, // checked against THEMES, which is defined below the defaults
};

function isAllowed(key, value) {
  if (key === 'theme') return THEMES.some((entry) => entry.id === value);
  const list = ALLOWED[key];
  return !list || list.includes(value);
}

function pickKnown(parsed, base) {
  const out = {};
  for (const key of Object.keys(base)) {
    if (!(key in parsed)) continue;
    if (typeof parsed[key] !== typeof base[key]) continue;
    // An enum read back from storage is validated as well as type-checked: a value
    // written by a newer build, or edited by hand, must not reach the renderer.
    if (typeof parsed[key] === 'string' && !isAllowed(key, parsed[key])) continue;
    out[key] = parsed[key];
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
  if (typeof value === 'string' && !isAllowed(key, value)) {
    throw new Error(`'${value}' is not a valid value for '${key}'`);
  }
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
