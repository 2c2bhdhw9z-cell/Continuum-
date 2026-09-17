/**
 * Per-game cheat lists.
 *
 * ## Where a cheat actually lives
 *
 * Two places, and keeping them straight is the whole design:
 *
 *   - **In the core**, as a numbered table pushed through `retro_cheat_set`. It is
 *     created when the session starts and dies with it, so it has to be re-pushed on
 *     every launch. Rust re-pushes it after a reset and after a state load too (see
 *     `EmulatorBridge::push_cheats`).
 *   - **Here**, in IndexedDB, as the durable list the user edits.
 *
 * This module owns the second and never talks to a core. The list is handed to Rust as a
 * whole, in order, because `retro_cheat_set` is indexed and removing the second of five
 * cheats would otherwise renumber the rest.
 *
 * ## Code formats
 *
 * Deliberately not validated here. Every system has its own convention — Game Genie
 * letter codes on the NES and SNES, `XXXXXXXX YYYY` Action Replay pairs on the GBA, raw
 * `address:value` on the Mega Drive — and cores accept several of them. A front end that
 * enforced one pattern would reject codes that work. So the code is stored as typed,
 * whitespace-normalised, and the core is left to accept or ignore it.
 *
 * What *is* enforced is the shape of the list: no blank codes, no duplicates, and a cap
 * so a paste accident cannot write a megabyte of "cheats".
 */

import { CHEATS, openDatabase, runTransaction, requestToPromise } from './idb.js';

/** Plenty for any real game; low enough that a runaway paste is caught. */
const MAX_PER_GAME = 128;
const MAX_CODE_LENGTH = 2048;

/** gameId -> list, populated by `hydrate()` so reads are synchronous for the UI. */
const byGame = new Map();
let hydrated = false;

/**
 * @typedef {{id: string, description: string, code: string, enabled: boolean}} Cheat
 */

/** Loads every stored list. One `getAll()`; the lists are small. */
export async function hydrate() {
  try {
    const db = await openDatabase();
    const rows =
      (await requestToPromise(
        db.transaction(CHEATS, 'readonly').objectStore(CHEATS).getAll(),
      )) ?? [];
    byGame.clear();
    let total = 0;
    for (const row of rows) {
      const list = Array.isArray(row.list) ? row.list : [];
      byGame.set(row.gameId, list);
      total += list.length;
    }
    hydrated = true;
    return { games: byGame.size, cheats: total };
  } catch (err) {
    console.warn('[cheats] could not restore cheat lists', err);
    hydrated = true;
    return { games: 0, cheats: 0 };
  }
}

export function isHydrated() {
  return hydrated;
}

/** Shared empty result so a miss allocates nothing on the render path. */
const EMPTY = Object.freeze([]);

/** @returns {Cheat[]} in the order the core will be given them. */
export function listFor(gameId) {
  return byGame.get(gameId) ?? EMPTY;
}

export function countFor(gameId) {
  return listFor(gameId).length;
}

export function activeCountFor(gameId) {
  let n = 0;
  for (const cheat of listFor(gameId)) if (cheat.enabled) n++;
  return n;
}

async function persist(gameId) {
  const list = byGame.get(gameId) ?? [];
  const db = await openDatabase();
  await runTransaction(db, [CHEATS], 'readwrite', (transaction) => {
    const store = transaction.objectStore(CHEATS);
    // An empty list is deleted rather than stored, so a game the user cleared does not
    // leave a row behind claiming it has cheats.
    if (list.length === 0) store.delete(gameId);
    else store.put({ gameId, list, updatedAt: Date.now() });
  });
}

/** Collapses runs of whitespace so "ABCD EFGH" and "ABCD  EFGH" are one cheat. */
function normaliseCode(code) {
  return code.trim().replace(/\s+/g, ' ');
}

/**
 * Adds a cheat.
 *
 * @param {string} gameId
 * @param {{description?: string, code: string, enabled?: boolean}} cheat
 * @returns {Promise<Cheat>}
 */
export async function add(gameId, { description = '', code, enabled = true }) {
  const normalised = normaliseCode(code ?? '');
  if (!normalised) throw new Error('a cheat needs a code');
  if (normalised.length > MAX_CODE_LENGTH) {
    throw new Error(`that code is ${normalised.length} characters long; ${MAX_CODE_LENGTH} is the limit`);
  }

  const list = byGame.get(gameId) ?? [];
  if (list.length >= MAX_PER_GAME) {
    throw new Error(`${MAX_PER_GAME} cheats per game is the limit`);
  }
  if (list.some((existing) => existing.code.toLowerCase() === normalised.toLowerCase())) {
    throw new Error('that code is already in the list');
  }

  const cheat = {
    id: `${gameId}:${Date.now().toString(36)}:${list.length}`,
    description: description.trim().slice(0, 120),
    code: normalised,
    enabled: Boolean(enabled),
  };
  byGame.set(gameId, [...list, cheat]);
  await persist(gameId);
  return cheat;
}

export async function remove(gameId, cheatId) {
  const list = byGame.get(gameId);
  if (!list) return false;
  const next = list.filter((cheat) => cheat.id !== cheatId);
  if (next.length === list.length) return false;
  byGame.set(gameId, next);
  await persist(gameId);
  return true;
}

export async function setEnabled(gameId, cheatId, enabled) {
  const list = byGame.get(gameId);
  if (!list) return false;
  const index = list.findIndex((cheat) => cheat.id === cheatId);
  if (index < 0) return false;
  // Replaced rather than mutated, so anything holding the old array sees a stable value.
  const next = list.slice();
  next[index] = { ...next[index], enabled: Boolean(enabled) };
  byGame.set(gameId, next);
  await persist(gameId);
  return true;
}

export async function clearFor(gameId) {
  byGame.delete(gameId);
  await persist(gameId);
}

/**
 * The list in the shape `EmulatorBridge::apply_cheats` wants: codes in order, plus a
 * parallel byte array of enabled flags.
 *
 * Disabled cheats are included. That is the libretro contract — a core keeps the code
 * and simply does not apply it — and it keeps indices stable, so toggling one cheat does
 * not renumber the others.
 *
 * @returns {{codes: string[], enabled: Uint8Array}}
 */
export function payloadFor(gameId) {
  const list = listFor(gameId);
  const codes = list.map((cheat) => cheat.code);
  const enabled = new Uint8Array(list.length);
  for (let i = 0; i < list.length; i++) enabled[i] = list[i].enabled ? 1 : 0;
  return { codes, enabled };
}

/** Drops every list. For "clear all storage", which empties the store too. */
export function resetIndex() {
  byGame.clear();
  hydrated = false;
}
