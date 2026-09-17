/**
 * Persistent ROM storage, on IndexedDB.
 *
 * Metadata and payloads live in separate object stores on purpose. Listing the
 * library must not deserialise megabytes of ROM data, and Phase 2's disc images are
 * hundreds of megabytes — a single-store design would make opening the library
 * proportional to the size of the collection rather than its length.
 *
 * ```text
 *   rom-meta : { id, name, systemId, extension, size, addedAt, sha1Prefix }
 *   rom-data : { id, bytes }
 * ```
 *
 * TODO(phase2): very large content should move to the Origin Private File System,
 * which streams instead of materialising the whole blob. The interface here is
 * already async and returns `Uint8Array`, so that swap does not reach callers.
 */

const DB_NAME = 'continuum';
const DB_VERSION = 1;
const META_STORE = 'rom-meta';
const DATA_STORE = 'rom-data';

/** @type {Promise<IDBDatabase>|null} */
let dbPromise = null;

function openDatabase() {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    if (!('indexedDB' in globalThis)) {
      reject(new Error('IndexedDB is unavailable, so imported ROMs cannot be saved'));
      return;
    }
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(META_STORE)) {
        const meta = db.createObjectStore(META_STORE, { keyPath: 'id' });
        meta.createIndex('systemId', 'systemId', { unique: false });
        meta.createIndex('addedAt', 'addedAt', { unique: false });
      }
      if (!db.objectStoreNames.contains(DATA_STORE)) {
        db.createObjectStore(DATA_STORE, { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error('could not open IndexedDB'));
  });
  return dbPromise;
}

/** Wraps a transaction in a promise that settles when it commits. */
function runTransaction(db, storeNames, mode, work) {
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(storeNames, mode);
    let result;
    transaction.oncomplete = () => resolve(result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error ?? new Error('transaction aborted'));
    result = work(transaction);
  });
}

function requestToPromise(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/**
 * Stores a ROM. Metadata and payload are written in one transaction, so a failure
 * cannot leave a library entry pointing at data that was never saved.
 *
 * @param {{id: string, name: string, systemId: string, extension: string, bytes: Uint8Array}} rom
 * @returns {Promise<object>} the stored metadata
 */
export async function putRom({ id, name, systemId, extension, bytes }) {
  const db = await openDatabase();
  const meta = {
    id,
    name,
    systemId,
    extension,
    size: bytes.length,
    addedAt: Date.now(),
  };
  await runTransaction(db, [META_STORE, DATA_STORE], 'readwrite', (transaction) => {
    transaction.objectStore(META_STORE).put(meta);
    // `slice()` detaches from any larger ArrayBuffer the File read produced, so the
    // stored record is exactly the ROM and nothing more.
    transaction.objectStore(DATA_STORE).put({ id, bytes: bytes.slice() });
    return meta;
  });
  return meta;
}

/** @returns {Promise<Uint8Array|null>} */
export async function getRomBytes(id) {
  const db = await openDatabase();
  const record = await requestToPromise(
    db.transaction(DATA_STORE, 'readonly').objectStore(DATA_STORE).get(id),
  );
  if (!record) return null;
  // Records round-trip as ArrayBuffer in some engines and Uint8Array in others.
  return record.bytes instanceof Uint8Array ? record.bytes : new Uint8Array(record.bytes);
}

/** @returns {Promise<object[]>} metadata only, newest first. */
export async function listRoms() {
  const db = await openDatabase();
  const all = await requestToPromise(
    db.transaction(META_STORE, 'readonly').objectStore(META_STORE).getAll(),
  );
  return (all ?? []).sort((a, b) => b.addedAt - a.addedAt);
}

export async function deleteRom(id) {
  const db = await openDatabase();
  await runTransaction(db, [META_STORE, DATA_STORE], 'readwrite', (transaction) => {
    transaction.objectStore(META_STORE).delete(id);
    transaction.objectStore(DATA_STORE).delete(id);
  });
}

/** Total bytes stored, for a settings screen. */
export async function usedBytes() {
  const roms = await listRoms();
  return roms.reduce((total, rom) => total + rom.size, 0);
}

/**
 * Stable id for a ROM, derived from its content rather than its filename.
 *
 * Two copies of the same ROM under different names should be one library entry, and
 * re-importing a file the user already has should not duplicate it. FNV-1a over the
 * header plus the length is not cryptographic, but for de-duplicating a personal
 * collection it is sufficient and it is fast on a 32 MB file.
 */
export function romId(bytes, extension) {
  let hash = 0x811c9dc5;
  const span = Math.min(bytes.length, 8192);
  for (let i = 0; i < span; i++) {
    hash ^= bytes[i];
    hash = (hash * 0x01000193) >>> 0;
  }
  // Mixing in the length separates ROMs that share a header (same mapper, same
  // publisher boilerplate) but differ in size.
  hash ^= bytes.length;
  hash = (hash * 0x01000193) >>> 0;
  return `rom-${extension || 'bin'}-${hash.toString(36)}-${bytes.length.toString(36)}`;
}
