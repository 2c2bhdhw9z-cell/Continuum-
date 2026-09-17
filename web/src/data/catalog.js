/**
 * The ROM catalogue.
 *
 * Phase 1 generates a large synthetic library — the UI has to be proven against
 * thousands of entries, and waiting for a user to import 4,000 ROMs before finding
 * out the list janks is not a plan. Generation is deterministic (seeded PRNG), so
 * every reload produces the identical library and a rendering bug is reproducible.
 *
 * ## Shape of the data, and why
 *
 * Entries live in one flat array. Every view then works with `Uint32Array`s of
 * *indices* into it — shelves, search results, filters. No view ever copies entry
 * objects, so switching from a 4,800-item grid to a 30-item search result allocates
 * one small typed array rather than rebuilding a list of objects.
 *
 * That is also what lets the virtual scrollers stay honest: `bindNode(node, i)`
 * resolves `indices[i]` and reads one object. O(1) per visible row, regardless of
 * catalogue size.
 *
 * TODO(phase1b): back this with IndexedDB + a file-import flow, keeping the same
 * index-array interface so the views need no changes. `entries` becomes a paged
 * window over the store; `shelves` and `search` become index queries.
 */

import { SYSTEMS, getSystem } from './systems.js';

/** Total synthetic entries. Large enough that O(n) DOM work would be obvious. */
const CATALOG_SIZE = 4800;

/** xorshift32 — small, fast, and reproducible across engines (unlike Math.random). */
function makeRandom(seed) {
  let state = seed | 0 || 0x9e3779b9;
  return function random() {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    // >>> 0 keeps it unsigned; /2^32 maps to [0, 1).
    return (state >>> 0) / 4294967296;
  };
}

const TITLE_A = [
  'Astro', 'Blaster', 'Chrono', 'Dragon', 'Echo', 'Frost', 'Galaxy', 'Hyper',
  'Iron', 'Jade', 'Kaiju', 'Lunar', 'Mecha', 'Neon', 'Omega', 'Phantom',
  'Quantum', 'Rogue', 'Solar', 'Turbo', 'Umbra', 'Vector', 'Warp', 'Xeno',
  'Zenith', 'Crystal', 'Shadow', 'Thunder', 'Cobalt', 'Crimson',
];

const TITLE_B = [
  'Knight', 'Racer', 'Legend', 'Quest', 'Force', 'Saga', 'Strike', 'Runner',
  'Empire', 'Warrior', 'Circuit', 'Odyssey', 'Rebellion', 'Chronicle', 'Fighter',
  'Command', 'Frontier', 'Requiem', 'Protocol', 'Arena', 'Dungeon', 'Tactics',
  'Adventure', 'Brigade', 'Horizon', 'Genesis', 'Reckoning', 'Dawn',
];

const TITLE_C = [
  '', '', '', '', ' II', ' III', ' IV', ' DX', ' Turbo', ' Advance', ' Zero',
  ' Deluxe', ' Remix', ' 2000', ' X', ' Gold', ': Reloaded', ': Rebirth',
];

const GENRES = [
  'Platformer', 'Shoot-em-up', 'JRPG', 'Racing', 'Fighting', 'Puzzle',
  'Action-adventure', 'Beat-em-up', 'Metroidvania', 'Strategy', 'Sports',
];

const REGIONS = ['NTSC-U', 'NTSC-J', 'PAL'];

const BLURB_OPENERS = [
  'A cult classic remembered for its soundtrack and punishing final act.',
  'Notorious for a difficulty curve that flattens the moment you learn to parry.',
  'Shipped late, sold poorly, and became a collector favourite a decade later.',
  'The sequel that quietly rebuilt every system from the first game.',
  'Held the speedrun record for a route its developers never intended.',
  'Beloved for its art direction and forgiven for its load times.',
  'A launch title that still shows off what the hardware could do.',
  'Localised twice, both times with a different ending.',
];

/** @typedef {{
 *   id: string, title: string, systemId: string, genre: string, year: number,
 *   region: string, players: number, rating: number, sizeMb: number,
 *   favorite: boolean, progress: number, lastPlayed: number | null, blurb: string,
 *   sortKey: string,
 * }} CatalogEntry */

/** @type {CatalogEntry[]} */
const entries = [];
/** Lowercased "title system genre year" haystack, parallel to `entries`. */
const searchHaystack = [];
const byId = new Map();
/** systemId -> Uint32Array of entry indices. */
const bySystem = new Map();

function generate() {
  const random = makeRandom(0x5eed1234);
  const phase1 = SYSTEMS.filter((s) => s.phase === 1);
  const now = Date.now();
  const perSystem = new Map(SYSTEMS.map((s) => [s.id, []]));

  for (let i = 0; i < CATALOG_SIZE; i++) {
    // Phase 2 systems get a thin presence: enough to show they exist in the UI,
    // not enough to dominate shelves the user cannot play yet.
    const usePhase2 = random() < 0.04;
    const pool = usePhase2 ? SYSTEMS.filter((s) => s.phase === 2) : phase1;
    const system = pool[Math.floor(random() * pool.length)];

    const a = TITLE_A[Math.floor(random() * TITLE_A.length)];
    const b = TITLE_B[Math.floor(random() * TITLE_B.length)];
    const c = TITLE_C[Math.floor(random() * TITLE_C.length)];
    const title = `${a} ${b}${c}`;

    const yearSpan = 8;
    const year = system.year + Math.floor(random() * yearSpan);
    const played = random();

    const entry = {
      id: `${system.id}-${i.toString(36)}`,
      title,
      systemId: system.id,
      genre: GENRES[Math.floor(random() * GENRES.length)],
      year,
      region: REGIONS[Math.floor(random() * REGIONS.length)],
      players: 1 + Math.floor(random() * 4),
      rating: Math.round((6 + random() * 4) * 10) / 10,
      sizeMb: Math.round((0.03 + random() * random() * 640) * 100) / 100,
      favorite: random() < 0.08,
      // 0 for never-started, otherwise partial progress.
      progress: played < 0.55 ? 0 : Math.round(random() * 100) / 100,
      lastPlayed:
        played < 0.55 ? null : now - Math.floor(random() * 90 * 86400_000),
      blurb: BLURB_OPENERS[Math.floor(random() * BLURB_OPENERS.length)],
      sortKey: '',
    };
    entry.sortKey = `${title.toLowerCase()}|${entry.id}`;

    entries.push(entry);
    searchHaystack.push(
      `${title} ${system.short} ${system.name} ${entry.genre} ${entry.year}`.toLowerCase(),
    );
    byId.set(entry.id, i);
    perSystem.get(system.id).push(i);
  }

  for (const [systemId, list] of perSystem) {
    list.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
    bySystem.set(systemId, Uint32Array.from(list));
  }
}

generate();

// ------------------------------------------------------------------ accessors

export const catalogSize = entries.length;

export function entryAt(index) {
  return entries[index];
}

export function entryById(id) {
  const index = byId.get(id);
  return index === undefined ? null : entries[index];
}

export function indexOfId(id) {
  return byId.has(id) ? byId.get(id) : -1;
}

/** Every index, title-sorted. Backing array for the all-games grid. */
const allSorted = (() => {
  const idx = new Uint32Array(entries.length);
  for (let i = 0; i < entries.length; i++) idx[i] = i;
  const arr = Array.from(idx);
  arr.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
  return Uint32Array.from(arr);
})();

export function allIndices() {
  return allSorted;
}

export function systemIndices(systemId) {
  return bySystem.get(systemId) ?? new Uint32Array(0);
}

export function favoriteIndices() {
  const out = [];
  for (let i = 0; i < entries.length; i++) if (entries[i].favorite) out.push(i);
  return Uint32Array.from(out);
}

export function toggleFavorite(id) {
  const index = byId.get(id);
  if (index === undefined) return false;
  entries[index].favorite = !entries[index].favorite;
  return entries[index].favorite;
}

/** Records a launch so "Continue playing" reflects reality. */
export function markPlayed(id) {
  const index = byId.get(id);
  if (index === undefined) return;
  const entry = entries[index];
  entry.lastPlayed = Date.now();
  if (entry.progress === 0) entry.progress = 0.01;
}

/**
 * Substring search over the prebuilt haystack.
 *
 * Linear over 4,800 short strings is well under a millisecond, and being an index
 * scan it produces a `Uint32Array` the scroller can consume directly. A trie or
 * inverted index only becomes worthwhile once this data lives in IndexedDB, at
 * which point the query moves into the store anyway.
 */
export function search(query, limit = 600) {
  const q = query.trim().toLowerCase();
  if (!q) return allSorted;
  const terms = q.split(/\s+/);
  const out = [];
  for (let i = 0; i < searchHaystack.length && out.length < limit; i++) {
    const hay = searchHaystack[i];
    let all = true;
    for (let t = 0; t < terms.length; t++) {
      if (!hay.includes(terms[t])) {
        all = false;
        break;
      }
    }
    if (all) out.push(i);
  }
  out.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
  return Uint32Array.from(out);
}

// --------------------------------------------------------------------- shelves

/**
 * Builds the Netflix-style rows.
 *
 * Recomputed on demand (after a launch, say) because "Continue playing" and
 * "Recently added" are time-dependent. Each shelf is metadata plus an index array;
 * the shelf itself renders lazily when it scrolls into view.
 *
 * @returns {{ id: string, title: string, indices: Uint32Array }[]}
 */
export function buildShelves() {
  const shelves = [];

  const continuing = [];
  for (let i = 0; i < entries.length; i++) {
    if (entries[i].lastPlayed !== null && entries[i].progress > 0) continuing.push(i);
  }
  continuing.sort((x, y) => entries[y].lastPlayed - entries[x].lastPlayed);
  if (continuing.length) {
    shelves.push({
      id: 'continue',
      title: 'Continue playing',
      indices: Uint32Array.from(continuing.slice(0, 40)),
    });
  }

  const favorites = favoriteIndices();
  if (favorites.length) {
    shelves.push({ id: 'favorites', title: 'Your favorites', indices: favorites });
  }

  const topRated = Array.from(allSorted);
  topRated.sort((x, y) => entries[y].rating - entries[x].rating);
  shelves.push({
    id: 'top-rated',
    title: 'Highest rated',
    indices: Uint32Array.from(topRated.slice(0, 60)),
  });

  // One shelf per system, biggest libraries first — mirrors how people browse.
  const systemsByCount = [...bySystem.entries()]
    .filter(([, list]) => list.length > 0)
    .sort((a, b) => b[1].length - a[1].length);

  for (const [systemId, indices] of systemsByCount) {
    const system = getSystem(systemId);
    if (!system) continue;
    shelves.push({
      id: `system-${systemId}`,
      title: system.phase === 2 ? `${system.name} (Phase 2)` : system.name,
      indices,
    });
  }

  return shelves;
}

/** Picks the hero title. Deterministic, so the banner does not flicker between reloads. */
export function featuredIndex() {
  let best = 0;
  let bestScore = -Infinity;
  for (let i = 0; i < entries.length; i++) {
    const e = entries[i];
    if (getSystem(e.systemId)?.phase !== 1) continue;
    // Favour a highly rated, unfinished game: the most plausible thing to feature.
    const score = e.rating * 2 + (e.progress > 0 ? 1 : 0) - e.year / 1000;
    if (score > bestScore) {
      bestScore = score;
      best = i;
    }
  }
  return best;
}
