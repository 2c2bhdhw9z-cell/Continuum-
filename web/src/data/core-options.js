/**
 * Stored core option values, per core.
 *
 * Per *core*, not per game — that is libretro's own model, and it is the right one: a
 * Game Boy palette or a GBA colour-correction setting is a preference about how you like
 * that hardware to look, not a property of one cartridge.
 *
 * localStorage rather than IndexedDB, for the same reason as `core-prefs.js` and
 * `settings.js`: these have to be readable synchronously at the moment a game launches,
 * and a database round-trip there would mean starting the core with its defaults and
 * then changing them a frame later.
 *
 * Nothing here validates a key or a value. The core is the authority on what it declares
 * — `WasmCore::set_core_option` reports a key the core does not know — so a stale setting
 * left over from an older core build is logged and skipped rather than being able to
 * wedge a launch.
 */

const STORAGE_KEY = 'continuum:core-options:v1';

/** @type {Record<string, Record<string, string>>|null} */
let cache = null;
let memoryOnly = false;

function readAll() {
  if (cache) return cache;
  if (memoryOnly) {
    cache = {};
    return cache;
  }
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = raw ? JSON.parse(raw) : null;
    cache = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
  } catch (err) {
    console.warn('[core-options] falling back to in-memory values:', err.message);
    memoryOnly = true;
    cache = {};
  }
  return cache;
}

function writeAll(next) {
  cache = next;
  if (memoryOnly) return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch (err) {
    console.warn('[core-options] could not persist values:', err.message);
    memoryOnly = true;
  }
}

/** @returns {Record<string, string>} stored values for one core, possibly empty. */
export function valuesFor(coreId) {
  const stored = readAll()[coreId];
  return stored && typeof stored === 'object' ? { ...stored } : {};
}

export function get(coreId, key) {
  return valuesFor(coreId)[key] ?? null;
}

/**
 * Records one value. Passing null clears it, which restores the core's own default.
 */
export function set(coreId, key, value) {
  const all = readAll();
  const forCore = { ...(all[coreId] ?? {}) };
  if (value === null || value === undefined) delete forCore[key];
  else forCore[key] = String(value);

  const next = { ...all };
  if (Object.keys(forCore).length === 0) delete next[coreId];
  else next[coreId] = forCore;
  writeAll(next);
}

/** Drops every stored value for one core. */
export function clearFor(coreId) {
  const next = { ...readAll() };
  delete next[coreId];
  writeAll(next);
}

export function clearAll() {
  writeAll({});
}

/** Every stored value, for the debug HUD and tests. */
export function all() {
  return structuredClone(readAll());
}
