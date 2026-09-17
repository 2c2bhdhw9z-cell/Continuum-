/**
 * The one IndexedDB connection, and the whole schema in one place.
 *
 * Two modules persist data — `rom-store.js` and `save-states.js` — and they must not
 * each open the database at a version of their own choosing. Two openers disagreeing
 * about `DB_VERSION` is not a subtle bug: the lower one is blocked indefinitely, and
 * whichever module happens to load second stops working. So the version and the
 * upgrade path live here, and both stores import them.
 *
 * ```text
 *   rom-meta    { id, name, systemId, extension, size, addedAt }
 *   rom-data    { id, bytes }
 *   state-meta  { id, gameId, slot, auto, createdAt, frame, sizeKb,
 *                 coreId, coreName, coreVersion, stateSize, contentId }
 *   state-data  { id, bytes }
 * ```
 *
 * Metadata and payloads are separate stores throughout. Listing a library, or a
 * game's save history, must not deserialise the payloads: a Mega Drive state is a
 * megabyte and a disc image is hundreds, so a single-store design makes opening a
 * list proportional to the *size* of the collection rather than its length.
 */

const DB_NAME = 'continuum';

/**
 * Bumped from 1 to 2 to add the save-state stores. Upgrades are additive and each
 * store creation is guarded, so an existing database with only the ROM stores gains
 * the new ones without touching what is already there.
 */
const DB_VERSION = 2;

export const ROM_META = 'rom-meta';
export const ROM_DATA = 'rom-data';
export const STATE_META = 'state-meta';
export const STATE_DATA = 'state-data';

/** @type {Promise<IDBDatabase>|null} */
let dbPromise = null;

export function openDatabase() {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    if (!('indexedDB' in globalThis)) {
      reject(new Error('IndexedDB is unavailable, so nothing can be saved'));
      return;
    }
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onupgradeneeded = () => {
      const db = request.result;

      if (!db.objectStoreNames.contains(ROM_META)) {
        const meta = db.createObjectStore(ROM_META, { keyPath: 'id' });
        meta.createIndex('systemId', 'systemId', { unique: false });
        meta.createIndex('addedAt', 'addedAt', { unique: false });
      }
      if (!db.objectStoreNames.contains(ROM_DATA)) {
        db.createObjectStore(ROM_DATA, { keyPath: 'id' });
      }

      if (!db.objectStoreNames.contains(STATE_META)) {
        const meta = db.createObjectStore(STATE_META, { keyPath: 'id' });
        // Every read is "the states for one game", so this index is the only way
        // listing stays proportional to that game's history and not the whole store.
        meta.createIndex('gameId', 'gameId', { unique: false });
        meta.createIndex('createdAt', 'createdAt', { unique: false });
      }
      if (!db.objectStoreNames.contains(STATE_DATA)) {
        db.createObjectStore(STATE_DATA, { keyPath: 'id' });
      }
    };

    request.onsuccess = () => {
      const db = request.result;
      // A second tab running a newer build will try to upgrade and be blocked by
      // this connection. Closing on `versionchange` lets it through; this tab's
      // next operation reopens at the new version.
      db.onversionchange = () => {
        db.close();
        dbPromise = null;
      };
      resolve(db);
    };
    request.onerror = () => reject(request.error ?? new Error('could not open IndexedDB'));
    request.onblocked = () =>
      reject(new Error('IndexedDB upgrade is blocked by another tab; close it and reload'));
  });
  return dbPromise;
}

/** Wraps a transaction in a promise that settles when it *commits*, not when the
 *  last request succeeds — otherwise a caller can observe a write that later aborts. */
export function runTransaction(db, storeNames, mode, work) {
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(storeNames, mode);
    let result;
    transaction.oncomplete = () => resolve(result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error ?? new Error('transaction aborted'));
    result = work(transaction);
  });
}

export function requestToPromise(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Records round-trip as ArrayBuffer in some engines and Uint8Array in others. */
export function toBytes(value) {
  if (!value) return null;
  return value instanceof Uint8Array ? value : new Uint8Array(value);
}

/**
 * Asks the browser not to evict this origin's storage under pressure.
 *
 * Worth requesting once at boot rather than never: iOS clears non-persistent storage
 * for sites the user has not visited in a week, which for a save-state store means
 * losing progress to inactivity. The request is granted silently for installed PWAs
 * and usually refused for a plain tab, so the return value is informational — there
 * is no fallback to arrange, only a fact to report in the HUD.
 *
 * @returns {Promise<boolean>} whether storage is now persistent
 */
export async function requestPersistentStorage() {
  try {
    if (!navigator.storage?.persist) return false;
    if (await navigator.storage.persisted()) return true;
    return await navigator.storage.persist();
  } catch {
    return false;
  }
}

/** @returns {Promise<{usage: number, quota: number}|null>} */
export async function storageEstimate() {
  try {
    if (!navigator.storage?.estimate) return null;
    const { usage = 0, quota = 0 } = await navigator.storage.estimate();
    return { usage, quota };
  } catch {
    return null;
  }
}
