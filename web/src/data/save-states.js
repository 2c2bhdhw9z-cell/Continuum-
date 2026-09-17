/**
 * Save-state index.
 *
 * In-memory for Phase 1, and deliberately generous with counts: a game the user
 * has been savescumming accumulates hundreds of states, which is exactly the case
 * that makes an unvirtualised list collapse. The detail sheet's list is therefore
 * built on the same `VirtualScroller` as the shelves.
 *
 * TODO(phase1b): persist to IndexedDB — metadata in an object store keyed by
 * `[gameId, slot]`, payloads as separate blobs so listing states never
 * deserialises megabytes of state data.
 */

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

/** Synthesises a plausible state history the first time a game is inspected. */
function seed(gameId) {
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
