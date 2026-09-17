/**
 * Content provider: hands the engine the bytes for a library entry.
 *
 * Two kinds of entry, and both are real:
 *
 *   - **imported** — a file the user chose. Bytes come from IndexedDB.
 *   - **builtin** — a cart shipped with the app. Fetched once and cached.
 *
 * There is no third kind. An earlier build generated thousands of placeholder entries
 * to prove the virtualiser at scale, which meant this module also had to synthesise
 * plausible-looking noise for them and every launch path needed a "does this actually
 * exist" branch. The placeholders are gone, so every entry in the library has bytes
 * behind it and `getContent` either returns them or throws.
 *
 * ROMs cannot be bundled with an emulator, so an empty library is the honest default
 * and importing is the primary path. The test carts are original ROMs written for this
 * project, which is what makes it legal to ship them and useful to keep them.
 */

import { getRomBytes } from './rom-store.js';
import { defaultExtension } from './systems.js';

/** Fetched built-ins, keyed by entry id. */
const cache = new Map();

/**
 * @param {import('./catalog.js').CatalogEntry} entry
 * @returns {Promise<Uint8Array>}
 */
export async function getContent(entry) {
  if (entry.source === 'builtin') {
    const cached = cache.get(entry.id);
    if (cached) return cached;
    const response = await fetch(entry.url, { cache: 'force-cache' });
    if (!response.ok) {
      throw new Error(`could not load ${entry.url}: ${response.status} ${response.statusText}`);
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    cache.set(entry.id, bytes);
    return bytes;
  }

  const bytes = await getRomBytes(entry.id);
  if (!bytes) {
    throw new Error(
      `"${entry.title}" is in the library but its data is missing from storage; re-import the file`,
    );
  }
  // Deliberately not cached: imported ROMs can be large and IndexedDB is fast enough,
  // so holding every launched ROM in memory would only grow the heap.
  return bytes;
}

/**
 * Filename to report to the core. Real cores resolve their content-info overrides
 * from the extension, so this is functional, not cosmetic.
 */
export function contentFilename(entry) {
  if (entry.filename) return entry.filename;
  const slug = entry.title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  return `${slug || 'content'}.${defaultExtension(entry.systemId)}`;
}
