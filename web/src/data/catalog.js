/**
 * The library index.
 *
 * Every entry here corresponds to real content: a cart that ships with the app, or a
 * file the user imported. There is no synthetic data and no placeholder rows — if a
 * title is on screen, its bytes are on disk and pressing Play runs it.
 *
 * ## Shape of the data, and why
 *
 * Entries live in one flat array. Every view then works with `Uint32Array`s of
 * *indices* into it — shelves, search results, filters. No view ever copies entry
 * objects, so switching from a shelf to a search result allocates one small typed
 * array rather than rebuilding a list of objects.
 *
 * That is also what lets the virtual scrollers stay honest: `bindNode(node, i)`
 * resolves `indices[i]` and reads one object. O(1) per visible row, regardless of
 * library size. The interface survived the removal of the 4,800-entry synthetic
 * catalogue unchanged, which is the whole reason the views needed no rewrite: a real
 * collection of a few thousand imported ROMs windows exactly the same way.
 *
 * ## Where the data comes from
 *
 * This module is an in-memory *index*, not the store. It is populated at boot by
 * `builtins.js` (four carts) and by `rom-import.js` reading IndexedDB. Mutations that
 * belong to the user rather than to the ROM — favourite, last played — are written
 * back through `rom-store.js` so they survive the app being closed.
 *
 * ## What is deliberately absent
 *
 * No rating, no genre, no player count, no review blurb. An emulator front end
 * cannot know any of that about a file it was handed, and inventing it produces a
 * library that looks informative and misleads. Fields here are either read out of the
 * file, parsed from its name, or recorded from what the user did.
 */

import { SYSTEMS, getSystem } from './systems.js';
import { putFlags } from './rom-store.js';

/** @typedef {{
 *   id: string, title: string, systemId: string, filename: string,
 *   sizeBytes: number, addedAt: number, source: 'imported'|'builtin',
 *   url?: string, region: string|null, tags: string[], blurb: string,
 *   favorite: boolean, lastPlayed: number|null, playCount: number,
 *   art: {kind: 'url'|'blob', url?: string, blob?: Blob, tier: string}|null,
 *   sortKey: string,
 * }} CatalogEntry */

/** @type {CatalogEntry[]} */
const entries = [];
/** Lowercased "title system" haystack, parallel to `entries`. */
const searchHaystack = [];
const byId = new Map();
/** systemId -> Uint32Array of entry indices. */
const bySystem = new Map();
/** @type {Uint32Array} */
let allSorted = new Uint32Array(0);

/** Region words that appear in No-Intro / GoodTools style filename tags. */
const REGION_WORDS = [
  'USA', 'Europe', 'Japan', 'World', 'Korea', 'China', 'Taiwan', 'Brazil',
  'Australia', 'Canada', 'France', 'Germany', 'Italy', 'Spain', 'Netherlands',
  'Sweden', 'Asia', 'PAL', 'NTSC',
];

/**
 * Pulls the parenthesised tags out of a filename-derived title.
 *
 * "Sonic The Hedgehog (USA, Europe) (Rev 1)" yields `['USA, Europe', 'Rev 1']`, from
 * which the region is whichever tag is made of region words. This is real information
 * that happens to be encoded in the name, so reading it is fair; guessing when it is
 * absent is not, and then `region` stays null and the UI omits the field.
 *
 * @param {string} title
 * @returns {{tags: string[], region: string|null}}
 */
export function parseTags(title) {
  const tags = [];
  for (const match of title.matchAll(/[([]([^)\]]+)[)\]]/g)) {
    const tag = match[1].trim();
    if (tag) tags.push(tag);
  }
  const region =
    tags.find((tag) =>
      tag
        .split(',')
        .map((part) => part.trim())
        .every((part) => REGION_WORDS.some((word) => word.toLowerCase() === part.toLowerCase())),
    ) ?? null;
  return { tags, region };
}

// ------------------------------------------------------------------ accessors

export function librarySize() {
  return entries.length;
}

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
export function allIndices() {
  return allSorted;
}

export function systemIndices(systemId) {
  return bySystem.get(systemId) ?? new Uint32Array(0);
}

export function favoriteIndices() {
  const out = [];
  for (let i = 0; i < entries.length; i++) if (entries[i].favorite) out.push(i);
  out.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
  return Uint32Array.from(out);
}

/** Systems that have at least one entry. Drives which shelves exist at all. */
export function systemsWithContent() {
  return SYSTEMS.filter((system) => (bySystem.get(system.id)?.length ?? 0) > 0);
}

// -------------------------------------------------------------------- mutation

/**
 * Recomputes the derived index arrays after entries change.
 *
 * A full rebuild rather than an in-place splice: this runs on import and on delete,
 * never on the scroll path, and sorting a few thousand short strings costs about a
 * millisecond. Correctness is worth more than the microseconds here.
 */
function rebuildDerivedIndexes() {
  const perSystem = new Map(SYSTEMS.map((s) => [s.id, []]));
  for (let i = 0; i < entries.length; i++) {
    const list = perSystem.get(entries[i].systemId);
    if (list) list.push(i);
  }
  for (const [systemId, list] of perSystem) {
    list.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
    bySystem.set(systemId, Uint32Array.from(list));
  }

  const all = Array.from({ length: entries.length }, (_, i) => i);
  all.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
  allSorted = Uint32Array.from(all);
}

/**
 * Registers content. Re-registering the same id updates it in place rather than
 * duplicating, so importing a file twice is a no-op and restoring from storage over
 * an already-populated index is safe.
 *
 * @param {{id: string, title: string, systemId: string, sizeBytes: number,
 *          filename: string, source: 'imported'|'builtin', url?: string,
 *          blurb?: string, addedAt?: number}} rom
 * @returns {CatalogEntry}
 */
export function addRealEntry(rom) {
  const existing = byId.get(rom.id);
  const previous = existing !== undefined ? entries[existing] : null;
  const system = getSystem(rom.systemId);
  const { tags, region } = parseTags(rom.title);

  const entry = {
    id: rom.id,
    title: rom.title,
    systemId: rom.systemId,
    filename: rom.filename,
    sizeBytes: rom.sizeBytes,
    addedAt: rom.addedAt ?? previous?.addedAt ?? Date.now(),
    source: rom.source,
    url: rom.url,
    region,
    tags,
    blurb:
      rom.blurb ??
      previous?.blurb ??
      `${rom.filename} · ${formatBytes(rom.sizeBytes)} · runs on the real ` +
        `${system?.name ?? rom.systemId} core.`,
    // User state is preserved across a re-register: re-importing a file must not
    // silently un-favourite it or forget that it was played.
    favorite: previous?.favorite ?? false,
    lastPlayed: previous?.lastPlayed ?? null,
    playCount: previous?.playCount ?? 0,
    art: previous?.art ?? null,
    sortKey: `${rom.title.toLowerCase()}|${rom.id}`,
  };

  const haystack = `${rom.title} ${system?.short ?? ''} ${system?.name ?? ''} ${
    region ?? ''
  }`.toLowerCase();

  if (existing !== undefined) {
    entries[existing] = entry;
    searchHaystack[existing] = haystack;
  } else {
    byId.set(rom.id, entries.length);
    entries.push(entry);
    searchHaystack.push(haystack);
  }

  rebuildDerivedIndexes();
  return entry;
}

/** Removes an entry (the ROM itself is deleted by the caller). */
export function removeEntry(id) {
  const index = byId.get(id);
  if (index === undefined) return false;
  entries.splice(index, 1);
  searchHaystack.splice(index, 1);
  byId.clear();
  for (let i = 0; i < entries.length; i++) byId.set(entries[i].id, i);
  rebuildDerivedIndexes();
  return true;
}

/** Drops every entry. Used by "clear all storage", which also empties the database. */
export function removeAllEntries() {
  entries.length = 0;
  searchHaystack.length = 0;
  byId.clear();
  rebuildDerivedIndexes();
}

/**
 * Applies flags read back from storage, without marking them dirty again.
 * @param {{id: string, favorite?: boolean, lastPlayed?: number|null, playCount?: number}} flags
 */
export function applyStoredFlags(flags) {
  const entry = entryById(flags.id);
  if (!entry) return false;
  entry.favorite = Boolean(flags.favorite);
  entry.lastPlayed = flags.lastPlayed ?? null;
  entry.playCount = flags.playCount ?? 0;
  return true;
}

/** Attaches artwork resolved by the scraper, a capture, or the user. */
export function setArtwork(id, art) {
  const entry = entryById(id);
  if (!entry) return false;
  entry.art = art;
  return true;
}

export function artworkFor(id) {
  return entryById(id)?.art ?? null;
}

/**
 * Flushes an entry's user state to IndexedDB.
 *
 * Fire-and-forget on purpose: a favourite toggle must feel instant, and the write is
 * a few bytes. A failure is logged rather than surfaced — losing a favourite is a
 * nuisance, and a modal about it would be worse than the problem.
 */
function persistFlags(entry) {
  void putFlags({
    id: entry.id,
    favorite: entry.favorite,
    lastPlayed: entry.lastPlayed,
    playCount: entry.playCount,
  }).catch((err) => console.warn('[library] could not persist flags', entry.id, err));
}

export function toggleFavorite(id) {
  const entry = entryById(id);
  if (!entry) return false;
  entry.favorite = !entry.favorite;
  persistFlags(entry);
  return entry.favorite;
}

/** Records a launch, so "Recently played" reflects reality across restarts. */
export function markPlayed(id) {
  const entry = entryById(id);
  if (!entry) return;
  entry.lastPlayed = Date.now();
  entry.playCount += 1;
  persistFlags(entry);
}

// ---------------------------------------------------------------------- search

/**
 * Substring search over the prebuilt haystack.
 *
 * Linear over a few thousand short strings is well under a millisecond, and being an
 * index scan it produces a `Uint32Array` the scroller can consume directly.
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
 * Builds the rows for the home view.
 *
 * **Shelves are strictly dynamic.** A shelf exists only when it has content, so a
 * fresh install shows exactly one row — the test carts — and a user who owns only
 * Game Boy games never sees an empty Mega Drive shelf. There is no scrolling past
 * rows of nothing to reach your own library.
 *
 * Recomputed on demand because "Recently played" is time-dependent. Each shelf is
 * metadata plus an index array; the shelf itself renders lazily when it scrolls into
 * view.
 *
 * @returns {{ id: string, title: string, subtitle?: string, indices: Uint32Array }[]}
 */
export function buildShelves() {
  const shelves = [];

  // Recently played first: for anyone with a library, this is the row they came for.
  const recent = [];
  for (let i = 0; i < entries.length; i++) if (entries[i].lastPlayed !== null) recent.push(i);
  recent.sort((x, y) => entries[y].lastPlayed - entries[x].lastPlayed);
  if (recent.length) {
    shelves.push({
      id: 'recent',
      title: 'Recently played',
      indices: Uint32Array.from(recent.slice(0, 40)),
    });
  }

  const favorites = favoriteIndices();
  if (favorites.length) {
    shelves.push({ id: 'favorites', title: 'Your favorites', indices: favorites });
  }

  // Newly imported, so a batch of files just added is immediately reachable without
  // hunting through per-system rows.
  const imported = [];
  for (let i = 0; i < entries.length; i++) if (entries[i].source === 'imported') imported.push(i);
  if (imported.length) {
    imported.sort((x, y) => entries[y].addedAt - entries[x].addedAt);
    shelves.push({
      id: 'recently-added',
      title: 'Recently added',
      indices: Uint32Array.from(imported.slice(0, 40)),
    });
  }

  // One shelf per system that actually has games, largest first — mirrors how people
  // browse, and a system with nothing in it contributes no row at all.
  const populated = [...bySystem.entries()]
    .filter(([, list]) => list.length > 0)
    .sort((a, b) => b[1].length - a[1].length);

  for (const [systemId, indices] of populated) {
    const system = getSystem(systemId);
    if (!system) continue;
    // Built-ins are collected into their own shelf below; a system whose only content
    // is its test cart would otherwise get a one-item row duplicating it.
    const hasImports = Array.from(indices).some((i) => entries[i].source === 'imported');
    if (!hasImports) continue;
    shelves.push({ id: `system-${systemId}`, title: system.name, indices });
  }

  // The carts that ship with the app, last: they are a demonstration, not a library.
  const builtins = [];
  for (let i = 0; i < entries.length; i++) if (entries[i].source === 'builtin') builtins.push(i);
  if (builtins.length) {
    builtins.sort((x, y) => entries[x].sortKey.localeCompare(entries[y].sortKey));
    shelves.push({
      id: 'test-carts',
      title: 'Continuum Test Carts',
      subtitle: 'Written for this project — one per core, each provably different',
      indices: Uint32Array.from(builtins),
    });
  }

  return shelves;
}

/**
 * Picks the hero title, or nothing when the library is empty.
 *
 * Prefers the most recently played, then the most recently added, then the
 * alphabetically first built-in.
 *
 * That last tie-break is load-bearing, not decoration. The four bundled carts are all
 * registered in the same tick, so their `addedAt` values differ by zero or one
 * millisecond depending on where the clock happens to tick — which meant the hero on a
 * fresh install was whichever cart won a race, and changed between reloads. Falling
 * back to `sortKey` makes the choice a property of the data rather than of the timing.
 *
 * @returns {number} an entry index, or -1 when there is nothing to feature
 */
export function featuredIndex() {
  if (!entries.length) return -1;

  let best = -1;
  for (let i = 0; i < entries.length; i++) {
    if (best === -1 || outranks(entries[i], entries[best])) best = i;
  }
  return best;
}

/** Strict "should `a` be the hero instead of `b`". */
function outranks(a, b) {
  // Tiers, so a played game always beats an unplayed one and an import always beats a
  // bundled test cart, regardless of timestamps.
  const tier = (e) => (e.lastPlayed !== null ? 3 : e.source === 'imported' ? 2 : 1);
  if (tier(a) !== tier(b)) return tier(a) > tier(b);

  const recency = (e) => e.lastPlayed ?? e.addedAt ?? 0;
  if (recency(a) !== recency(b)) return recency(a) > recency(b);

  return a.sortKey.localeCompare(b.sortKey) < 0;
}

/** "24.0 KB" / "4.2 MB". Shared by blurbs and the metadata line. */
export function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 KB';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
