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
  ROM_ART,
  ENTRY_FLAGS,
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
 * The save states, the artwork and the library flags go in the same transaction.
 * Sharing one database is what makes that possible: deleting a ROM but leaving its
 * states behind would orphan megabytes that nothing in the UI can ever reach or
 * remove.
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
    [META_STORE, DATA_STORE, STATE_META, STATE_DATA, ROM_ART, ENTRY_FLAGS],
    'readwrite',
    (transaction) => {
      transaction.objectStore(META_STORE).delete(id);
      transaction.objectStore(DATA_STORE).delete(id);
      transaction.objectStore(ROM_ART).delete(id);
      transaction.objectStore(ENTRY_FLAGS).delete(id);
      for (const stateId of stateIds ?? []) {
        transaction.objectStore(STATE_META).delete(stateId);
        transaction.objectStore(STATE_DATA).delete(stateId);
      }
    },
  );
  return (stateIds ?? []).length;
}

/** Total ROM bytes stored, for the Settings screen. */
export async function usedBytes() {
  const roms = await listRoms();
  return roms.reduce((total, rom) => total + rom.size, 0);
}

// ------------------------------------------------------------------- cover art

/**
 * Stores artwork for an entry.
 *
 * Two kinds, and the difference is forced on us rather than chosen:
 *
 *   - `url` — a libretro thumbnail. That host serves no `Access-Control-Allow-Origin`
 *     header, so script cannot read the bytes: `fetch` is refused and drawing the
 *     image to a canvas would taint it. An `<img>` renders it perfectly well, so what
 *     gets persisted is the resolved address.
 *   - `blob` — artwork we produced ourselves (an in-game capture) or that the user
 *     picked off their device. Same-origin or local, so the bytes are ours to keep,
 *     and this kind survives with no network at all.
 *
 * @param {{id: string, kind: 'url'|'blob', url?: string, blob?: Blob, tier: string}} art
 */
export async function putArt({ id, kind, url, blob, tier }) {
  const db = await openDatabase();
  const record = { id, kind, tier, updatedAt: Date.now() };
  if (kind === 'url') record.url = url;
  else record.blob = blob;
  await runTransaction(db, [ROM_ART], 'readwrite', (transaction) => {
    transaction.objectStore(ROM_ART).put(record);
  });
  return record;
}

/** @returns {Promise<object[]>} every art record, for hydrating the library at boot. */
export async function listArt() {
  const db = await openDatabase();
  return (
    (await requestToPromise(
      db.transaction(ROM_ART, 'readonly').objectStore(ROM_ART).getAll(),
    )) ?? []
  );
}

export async function deleteArt(id) {
  const db = await openDatabase();
  await runTransaction(db, [ROM_ART], 'readwrite', (transaction) => {
    transaction.objectStore(ROM_ART).delete(id);
  });
}

// -------------------------------------------------------------- library flags

/**
 * Persists the per-entry state that is the user's, not the ROM's: favourite,
 * last played, play count.
 *
 * Without this the Favorites shelf is a lie the moment the app is closed — the
 * whole point of populating the library from storage is that what you left is what
 * you come back to.
 *
 * @param {{id: string, favorite?: boolean, lastPlayed?: number|null, playCount?: number}} flags
 */
export async function putFlags(flags) {
  const db = await openDatabase();
  await runTransaction(db, [ENTRY_FLAGS], 'readwrite', (transaction) => {
    transaction.objectStore(ENTRY_FLAGS).put({
      id: flags.id,
      favorite: Boolean(flags.favorite),
      lastPlayed: flags.lastPlayed ?? null,
      playCount: flags.playCount ?? 0,
    });
  });
}

/** @returns {Promise<object[]>} */
export async function listFlags() {
  const db = await openDatabase();
  return (
    (await requestToPromise(
      db.transaction(ENTRY_FLAGS, 'readonly').objectStore(ENTRY_FLAGS).getAll(),
    )) ?? []
  );
}

// ------------------------------------------------------------------ statistics

/**
 * What is actually on disk, counted rather than estimated.
 *
 * `navigator.storage.estimate()` is reported alongside these numbers but cannot
 * replace them: it covers the whole origin including the service worker's cache of
 * the app shell and the core binaries, so quoting it as "your ROMs" would tell
 * someone their empty library is using 12 MB.
 *
 * State payload sizes come from `state-meta`, which is the reason payloads live in a
 * store of their own — adding up a hundred `stateSize` fields reads kilobytes, where
 * `getAll()` on `state-data` would deserialise every megabyte to measure it.
 *
 * @returns {Promise<{romCount: number, romBytes: number, stateCount: number,
 *   stateBytes: number, artCount: number, artBytes: number}>}
 */
export async function storageBreakdown() {
  const db = await openDatabase();
  const [roms, states, art] = await Promise.all([
    requestToPromise(db.transaction(META_STORE, 'readonly').objectStore(META_STORE).getAll()),
    requestToPromise(db.transaction(STATE_META, 'readonly').objectStore(STATE_META).getAll()),
    requestToPromise(db.transaction(ROM_ART, 'readonly').objectStore(ROM_ART).getAll()),
  ]);

  const romList = roms ?? [];
  const stateList = states ?? [];
  const artList = art ?? [];

  return {
    romCount: romList.length,
    romBytes: romList.reduce((total, rom) => total + (rom.size ?? 0), 0),
    stateCount: stateList.length,
    // `stateSize` is the uncompressed payload length recorded when the state was
    // written; `sizeKb` is the same number rounded for display.
    stateBytes: stateList.reduce(
      (total, state) => total + (state.stateSize ?? Math.round((state.sizeKb ?? 0) * 1024)),
      0,
    ),
    artCount: artList.length,
    artBytes: artList.reduce((total, record) => total + (record.blob?.size ?? 0), 0),
  };
}

/**
 * Empties every store this app owns.
 *
 * One transaction over all six, so there is no window in which the ROMs are gone but
 * their save states remain — a half-cleared database would leave the library showing
 * entries whose bytes no longer exist.
 *
 * @returns {Promise<{roms: number, states: number}>} what was removed
 */
export async function clearEverything() {
  const db = await openDatabase();
  const before = await storageBreakdown();
  await runTransaction(
    db,
    [META_STORE, DATA_STORE, STATE_META, STATE_DATA, ROM_ART, ENTRY_FLAGS],
    'readwrite',
    (transaction) => {
      for (const store of [
        META_STORE,
        DATA_STORE,
        STATE_META,
        STATE_DATA,
        ROM_ART,
        ENTRY_FLAGS,
      ]) {
        transaction.objectStore(store).clear();
      }
    },
  );
  return { roms: before.romCount, states: before.stateCount };
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
