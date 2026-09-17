/**
 * Save-state index.
 *
 * Synthetic histories are generated **only for synthetic catalogue entries**, and
 * deliberately generous with counts: a game someone has been savescumming
 * accumulates hundreds of states, which is exactly the case that makes an
 * unvirtualised list collapse. The detail sheet's list is built on the same
 * `VirtualScroller` as the shelves so that case costs nothing.
 *
 * Real content — an imported ROM, or the built-in test cart — starts empty. Showing
 * invented save states for a ROM the user actually owns would be a lie the UI tells
 * about their data, and the "Load" button would then fail on states that never
 * existed.
 *
 * TODO(phase1c): persist real states to IndexedDB — metadata in one object store,
 * payloads in another, so listing never deserialises megabytes of state data. The
 * in-memory payloads `player-view.js` keeps for the current session are the shape
 * that store needs.
 */

import { entryById } from './catalog.js';

/** @typedef {{ slot: number, createdAt: number, frame: number, sizeKb: number, auto: boolean }} SaveState */

/** @type {Map<string, SaveState[]>} */
const byGame = new Map();

function makeRandom(seedText) {
  let state = 0x2545f491;
  for (let i = 0; i < seedText.length; i++) {
    state = (state ^ seedText.charCodeAt(i)) >>> 0;
    state = (state * 0x01000193) >>> 0;
  }
  return function random() {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 4294967296;
  };
}

/**
 * Builds the initial history the first time a game is inspected: empty for real
 * content, a plausible synthetic history for catalogue placeholders.
 */
function seed(gameId) {
  if (entryById(gameId)?.real) {
    const states = [];
    byGame.set(gameId, states);
    return states;
  }

  const random = makeRandom(gameId);
  // Most games have none; a few have a lot. That distribution is the point.
  const count =
    random() < 0.45 ? 0 : Math.floor(random() * random() * 220) + 1;
  const now = Date.now();
  const states = [];
  for (let i = 0; i < count; i++) {
    states.push({
      slot: i,
      createdAt: now - Math.floor(random() * 45 * 86400_000),
      frame: Math.floor(random() * 900_000),
      sizeKb: Math.round((8 + random() * 3200) * 10) / 10,
      auto: random() < 0.3,
    });
  }
  states.sort((a, b) => b.createdAt - a.createdAt);
  byGame.set(gameId, states);
  return states;
}

/** @returns {SaveState[]} newest first. */
export function listFor(gameId) {
  return byGame.get(gameId) ?? seed(gameId);
}

export function countFor(gameId) {
  return listFor(gameId).length;
}

/** Records a new state. Returns the created record. */
export function add(gameId, { frame, sizeKb, auto = false }) {
  const states = listFor(gameId);
  const slot = states.length ? Math.max(...states.map((s) => s.slot)) + 1 : 0;
  const record = { slot, createdAt: Date.now(), frame, sizeKb, auto };
  states.unshift(record);
  return record;
}

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
