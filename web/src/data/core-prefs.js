/**
 * Which core the user wants for each system.
 *
 * A system maps to more than one core — `gb` runs on mGBA or Gambatte, and neither
 * answer is wrong — so the choice belongs to the user and has to outlive a reload.
 * The preference is stored per *system*, not per game: someone who prefers a
 * particular Game Boy core prefers it for their whole Game Boy library.
 *
 * Nothing here validates the stored id. That is deliberate: the Rust registry is the
 * only thing that knows which cores exist and what they can run, so it does the
 * checking (see `CoreRegistry::resolve_core_for_system`, which ignores a preference
 * that no longer applies). This module's only job is durable storage, which means a
 * manifest change can never leave a game unlaunchable because of a stale string here.
 */

const STORAGE_KEY = 'continuum:core-prefs:v1';

/**
 * localStorage throws rather than degrades in several real situations — Safari
 * private browsing, storage disabled by policy, quota exhausted. A core preference is
 * a convenience, so every access falls back to in-memory state instead of taking the
 * app down with it.
 */
let memoryFallback = null;

function readAll() {
  if (memoryFallback) return memoryFallback;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    // Anything other than a flat object is treated as corrupt and discarded.
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return parsed;
  } catch (err) {
    console.warn('[core-prefs] falling back to in-memory preferences:', err.message);
    memoryFallback = {};
    return memoryFallback;
  }
}

function writeAll(prefs) {
  if (memoryFallback) {
    memoryFallback = prefs;
    return;
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch (err) {
    console.warn('[core-prefs] could not persist preferences:', err.message);
    memoryFallback = prefs;
  }
}

/**
 * The core id the user chose for `systemId`, or null for "use the default".
 * @param {string} systemId
 * @returns {string|null}
 */
export function getCorePreference(systemId) {
  const value = readAll()[systemId];
  return typeof value === 'string' && value ? value : null;
}

/**
 * Records a choice. Passing null clears it, restoring the manifest default.
 * @param {string} systemId
 * @param {string|null} coreId
 */
export function setCorePreference(systemId, coreId) {
  const prefs = { ...readAll() };
  if (coreId) prefs[systemId] = coreId;
  else delete prefs[systemId];
  writeAll(prefs);
}

/** All stored choices, for the debug HUD and tests. */
export function allCorePreferences() {
  return { ...readAll() };
}

/** Drops every stored choice. */
export function clearCorePreferences() {
  writeAll({});
}
