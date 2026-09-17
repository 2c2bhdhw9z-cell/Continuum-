/**
 * Save states: an index in memory, payloads in IndexedDB.
 *
 * ## Why the index is synchronous
 *
 * The detail sheet's list is a `VirtualScroller`, and `bindNode` runs on the scroll
 * path — it cannot await anything. So the *metadata* for every state is hydrated once
 * at boot and kept in memory, where `listFor()` can answer synchronously, while the
 * payloads stay on disk until someone actually loads one. Metadata is a few dozen
 * bytes per state; payloads are 13 KB for the NES and a megabyte for the Mega Drive,
 * which is the whole reason for the split.
 *
 * ## Compatibility
 *
 * A libretro save state is an opaque dump of a core's internal structs. It is only
 * meaningful to the *same build* of the *same core*: `retro_unserialize` is not
 * versioned, and handing it a foreign blob does not reliably fail — it can succeed
 * into a corrupted machine that crashes minutes later somewhere unrelated. So every
 * record carries the core's id, its self-reported version and the exact byte length,
 * and `compatibility()` refuses anything that does not match rather than hoping the
 * core notices. See `describeIncompatibility()` for what the user is told.
 *
 * ## Auto-saves
 *
 * Each game gets one auto-save, keyed `<gameId>:auto`, overwritten in place. Manual
 * saves take numbered slots and accumulate. Keeping them in separate key namespaces
 * means an auto-save can never consume a slot number the user was using, and the
 * resume path never has to guess which of several records is the newest.
 *
 * ## Every record here is real
 *
 * An earlier build fabricated a plausible save history for catalogue placeholders, so
 * that a game with 200 states existed to prove the list virtualisation against. The
 * placeholders are gone and so is the generator: every record in this index was
 * written by a core, has a payload on disk, and can be loaded. The virtualised list
 * still handles 200 states — it is the same code — but it now has to be reached by
 * actually saving 200 times, which is the honest version of that claim.
 */

import {
  STATE_META,
  STATE_DATA,
  openDatabase,
  runTransaction,
  requestToPromise,
  toBytes,
} from './idb.js';

/**
 * @typedef {object} SaveState
 * @property {string} id           `<gameId>:<slot>` or `<gameId>:auto`
 * @property {string} gameId
 * @property {number} slot         auto-saves use -1, which never collides
 * @property {boolean} auto
 * @property {number} createdAt
 * @property {number} frame        emulated frame the state was taken at
 * @property {number} sizeKb
 * @property {string} [coreId]
 * @property {string} [coreName]
 * @property {string} [coreVersion]
 * @property {number} [stateSize]  payload length in bytes
 */

/** Auto-saves are not in the numbered sequence, so they get a slot outside it. */
export const AUTO_SLOT = -1;

/** @type {Map<string, SaveState[]>} gameId -> newest first */
const byGame = new Map();

let hydrated = false;

function stateId(gameId, slot) {
  return slot === AUTO_SLOT ? `${gameId}:auto` : `${gameId}:${slot}`;
}

function sortNewestFirst(states) {
  states.sort((a, b) => b.createdAt - a.createdAt);
  return states;
}

// ------------------------------------------------------------------ hydration

/**
 * Loads every state's metadata into memory. Call once, before the first render.
 *
 * Payloads are deliberately not read. A library with fifty states across four
 * systems is tens of megabytes of payload and a few kilobytes of metadata; reading
 * the former to draw a list would make boot time scale with how much someone has
 * played.
 *
 * @returns {Promise<{states: number, games: number}>}
 */
export async function hydrate() {
  if (hydrated) return summary();
  try {
    const db = await openDatabase();
    const all = await requestToPromise(
      db.transaction(STATE_META, 'readonly').objectStore(STATE_META).getAll(),
    );
    for (const record of all ?? []) {
      const list = byGame.get(record.gameId);
      if (list) list.push(record);
      else byGame.set(record.gameId, [record]);
    }
    for (const list of byGame.values()) sortNewestFirst(list);
  } catch (err) {
    // Storage being unavailable must not stop the app from running; it only means
    // states cannot outlive the session.
    console.warn('[states] could not hydrate the save-state index:', err.message);
  }
  hydrated = true;
  return summary();
}

function summary() {
  let states = 0;
  for (const list of byGame.values()) states += list.length;
  return { states, games: byGame.size };
}

export function isHydrated() {
  return hydrated;
}

// ------------------------------------------------------------------- reading

/** @returns {SaveState[]} newest first. Synchronous by design — see the module note. */
export function listFor(gameId) {
  return byGame.get(gameId) ?? EMPTY;
}

/** Shared empty result, so a miss allocates nothing on the scroll path. */
const EMPTY = Object.freeze([]);

export function countFor(gameId) {
  return listFor(gameId).length;
}

/** The auto-save for a game, if it has one. */
export function autoStateFor(gameId) {
  return listFor(gameId).find((s) => s.auto) ?? null;
}

/** Manual saves only, newest first. */
export function manualStatesFor(gameId) {
  return listFor(gameId).filter((s) => !s.auto);
}

/**
 * Reads a payload back off disk.
 * @returns {Promise<Uint8Array|null>} null when the payload is missing, which the
 *   caller must treat as "cannot load", not as empty.
 */
export async function payloadFor(gameId, slot) {
  const record = listFor(gameId).find((s) => s.slot === slot);
  if (!record) return null;
  try {
    const db = await openDatabase();
    const row = await requestToPromise(
      db.transaction(STATE_DATA, 'readonly').objectStore(STATE_DATA).get(record.id),
    );
    return toBytes(row?.bytes);
  } catch (err) {
    console.warn('[states] could not read payload:', err.message);
    return null;
  }
}

// ---------------------------------------------------------------- compatibility

/**
 * Whether a state can safely be given to the core that is running now.
 *
 * Deliberately strict. The failure mode being avoided is not "load refused", it is
 * `retro_unserialize` accepting a blob it should not and leaving the machine subtly
 * wrong — a corrupted game that crashes ten minutes later, with nothing to connect it
 * back to the state that caused it.
 *
 * @param {SaveState} record
 * @param {{coreId: string, coreVersion?: string, stateSize?: number}} current
 * @returns {{ok: boolean, reason?: string, detail?: string}}
 */
export function compatibility(record, current) {
  if (!record) return { ok: false, reason: 'missing' };
  if (record.coreId && current.coreId && record.coreId !== current.coreId) {
    return {
      ok: false,
      reason: 'core',
      detail: `saved by ${record.coreName ?? record.coreId}, now running ${current.coreId}`,
    };
  }
  if (
    record.coreVersion &&
    current.coreVersion &&
    record.coreVersion !== current.coreVersion
  ) {
    return {
      ok: false,
      reason: 'version',
      detail: `saved by ${record.coreName ?? record.coreId} ${record.coreVersion}, ` +
        `this build is ${current.coreVersion}`,
    };
  }
  // The last line of defence, and the one that catches a core whose state layout
  // changed without its version string changing.
  if (record.stateSize && current.stateSize && record.stateSize !== current.stateSize) {
    return {
      ok: false,
      reason: 'size',
      detail: `state is ${record.stateSize} bytes, this core expects ${current.stateSize}`,
    };
  }
  return { ok: true };
}

/** A sentence to show the user. Never blames them, and never says "unknown error". */
export function describeIncompatibility(result) {
  switch (result.reason) {
    case 'missing':
      return 'That state is no longer stored.';
    case 'core':
      return `A state can only be loaded by the core that wrote it — ${result.detail}.`;
    case 'version':
      return `The core has been rebuilt since this state was saved (${result.detail}), and its internal layout may have moved. Not loading it, to avoid corrupting the game.`;
    case 'size':
      return `This state does not match what the core expects (${result.detail}), so it was almost certainly written by a different build.`;
    default:
      return 'That state cannot be loaded.';
  }
}

// ------------------------------------------------------------------- writing

/**
 * Persists a state. Metadata and payload go in one transaction, so a listed state
 * can never point at data that was never written.
 *
 * @param {object} args
 * @param {string} args.gameId
 * @param {Uint8Array} args.bytes
 * @param {number} args.frame
 * @param {boolean} [args.auto]
 * @param {{id: string, name?: string, version?: string}} args.core
 * @param {string} [args.contentId]
 * @returns {Promise<SaveState>}
 */
export async function put({ gameId, bytes, frame, auto = false, core, contentId }) {
  // `listFor` returns a shared frozen empty array for a game with no history, so the
  // mutable list has to be materialised before anything is appended to it.
  let list = byGame.get(gameId);
  if (!list) {
    list = [];
    byGame.set(gameId, list);
  }

  const slot = auto ? AUTO_SLOT : nextSlot(list);
  const record = {
    id: stateId(gameId, slot),
    gameId,
    slot,
    auto,
    createdAt: Date.now(),
    frame: Math.round(frame),
    sizeKb: Math.round((bytes.length / 1024) * 10) / 10,
    coreId: core.id,
    coreName: core.name ?? core.id,
    coreVersion: core.version ?? '',
    stateSize: bytes.length,
    contentId: contentId ?? gameId,
  };

  const db = await openDatabase();
  try {
    await runTransaction(db, [STATE_META, STATE_DATA], 'readwrite', (transaction) => {
      transaction.objectStore(STATE_META).put(record);
      // `slice()` detaches the bytes from whatever larger buffer produced them, so
      // the stored record is exactly the state and nothing more.
      transaction.objectStore(STATE_DATA).put({ id: record.id, bytes: bytes.slice() });
    });
  } catch (err) {
    if (err?.name === 'QuotaExceededError') {
      // Make room from the oldest manual states and try once more. Auto-saves are
      // never evicted: they are the thing standing between the user and lost
      // progress, which is exactly what they would want kept.
      const freed = await evictOldest(bytes.length);
      if (freed > 0) {
        await runTransaction(db, [STATE_META, STATE_DATA], 'readwrite', (transaction) => {
          transaction.objectStore(STATE_META).put(record);
          transaction.objectStore(STATE_DATA).put({ id: record.id, bytes: bytes.slice() });
        });
      } else {
        throw new Error('storage is full and there are no old states left to remove');
      }
    } else {
      throw err;
    }
  }

  // In-memory index last, so it only ever describes what is actually on disk.
  const existing = list.findIndex((s) => s.slot === slot);
  if (existing >= 0) list.splice(existing, 1);
  list.push(record);
  sortNewestFirst(list);
  return record;
}

function nextSlot(list) {
  const numbered = list.filter((s) => s.slot >= 0).map((s) => s.slot);
  return numbered.length ? Math.max(...numbered) + 1 : 0;
}

/**
 * Deletes oldest-first until at least `needed` bytes have been reclaimed.
 * @returns {Promise<number>} bytes freed
 */
async function evictOldest(needed) {
  const candidates = [];
  for (const list of byGame.values()) {
    for (const record of list) {
      if (!record.auto) candidates.push(record);
    }
  }
  candidates.sort((a, b) => a.createdAt - b.createdAt);

  let freed = 0;
  for (const record of candidates) {
    if (freed >= needed) break;
    await remove(record.gameId, record.slot);
    freed += record.stateSize ?? 0;
    console.info(`[states] evicted ${record.id} to make room (${freed} bytes freed)`);
  }
  return freed;
}

export async function remove(gameId, slot) {
  const list = listFor(gameId);
  const index = list.findIndex((s) => s.slot === slot);
  if (index < 0) return false;
  const record = list[index];
  const db = await openDatabase();
  await runTransaction(db, [STATE_META, STATE_DATA], 'readwrite', (transaction) => {
    transaction.objectStore(STATE_META).delete(record.id);
    transaction.objectStore(STATE_DATA).delete(record.id);
  });
  list.splice(index, 1);
  return true;
}

/** Drops a game's auto-save, so the next launch starts from the beginning. */
export async function clearAuto(gameId) {
  return remove(gameId, AUTO_SLOT);
}

/** Total state bytes held, for the Settings screen and the HUD. */
export function usedBytes() {
  let total = 0;
  for (const list of byGame.values()) {
    for (const record of list) total += record.stateSize ?? 0;
  }
  return total;
}

// -------------------------------------------------------------------- display

/** Human-friendly relative time. Keeps the row renderer free of date logic. */
export function formatAge(timestamp) {
  const seconds = Math.max(1, Math.round((Date.now() - timestamp) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(timestamp).toLocaleDateString();
}

/** Test seam: forget everything without touching disk. */
/**
 * Drops the in-memory index.
 *
 * Two real callers: the smoke test, which discards it to prove the index rehydrates
 * from disk, and "clear all storage" in Settings, which has just emptied the object
 * stores this index describes. Leaving it populated after that would have the UI
 * listing save states whose payloads no longer exist.
 *
 * `hydrated` is reset too, so the next read re-reads the database rather than trusting
 * an index built from data that is gone.
 */
export function resetIndex() {
  byGame.clear();
  hydrated = false;
}

