/**
 * Persistent ROM storage, on IndexedDB.
 *
 * The connection, the schema and the version live in `idb.js`, shared with
 * `save-states.js`: two modules opening the same database at different versions
 * deadlock, so there is exactly one opener.
 *
 * ```text
 *   rom-meta : { id, name, systemId, extension, size, addedAt }
 *   rom-data : { id, bytes }
 * ```
 *
 * TODO(phase2): very large content should move to the Origin Private File System,
 * which streams instead of materialising the whole blob. The interface here is
 * already async and returns `Uint8Array`, so that swap does not reach callers.
 */

import {
  ROM_META as META_STORE,
  ROM_DATA as DATA_STORE,
  STATE_META,
  STATE_DATA,
  openDatabase,
  runTransaction,
  requestToPromise,
  toBytes,
} from './idb.js';

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
  return toBytes(record.bytes);
}

/** @returns {Promise<object[]>} metadata only, newest first. */
export async function listRoms() {
  const db = await openDatabase();
  const all = await requestToPromise(
    db.transaction(META_STORE, 'readonly').objectStore(META_STORE).getAll(),
  );
  return (all ?? []).sort((a, b) => b.addedAt - a.addedAt);
}

/**
 * Deletes a ROM and everything saved against it.
 *
 * The save states go in the same transaction. Sharing one database is what makes
 * that possible: deleting a ROM but leaving its states behind would orphan megabytes
 * that nothing in the UI can ever reach or remove.
 */
export async function deleteRom(id) {
  const db = await openDatabase();
  const stateIds = await requestToPromise(
    db
      .transaction(STATE_META, 'readonly')
      .objectStore(STATE_META)
      .index('gameId')
      .getAllKeys(id),
  );
  await runTransaction(
    db,
    [META_STORE, DATA_STORE, STATE_META, STATE_DATA],
    'readwrite',
    (transaction) => {
      transaction.objectStore(META_STORE).delete(id);
      transaction.objectStore(DATA_STORE).delete(id);
      for (const stateId of stateIds ?? []) {
        transaction.objectStore(STATE_META).delete(stateId);
        transaction.objectStore(STATE_DATA).delete(stateId);
      }
    },
  );
  return (stateIds ?? []).length;
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
