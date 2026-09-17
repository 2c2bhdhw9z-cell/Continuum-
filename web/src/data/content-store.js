/**
 * Content provider: hands the engine the bytes for a library entry.
 *
 * Three kinds of entry, and the difference matters:
 *
 *   - **imported** — a file the user chose. Bytes come from IndexedDB. Plays.
 *   - **builtin** — content shipped with the app (the NES test cart). Fetched once
 *     and cached. Plays.
 *   - **synthetic** — a catalogue placeholder generated to exercise the UI at scale.
 *     There is no ROM behind it, so a real core will reject it, and
 *     [`isPlayable`] returns false so the launch path can say why instead of
 *     surfacing "content rejected" from deep inside a core.
 *
 * ROMs cannot be bundled with an emulator, so an empty library is the honest default
 * and importing is the primary path. The test cart exists so the pipeline can be
 * demonstrated without one.
 */

import { getRomBytes } from './rom-store.js';
import { defaultExtension } from './systems.js';

/** Placeholder payload size for synthetic entries. Non-empty, and nothing more. */
const PLACEHOLDER_BYTES = 64 * 1024;

/** Fetched built-ins and synthesised buffers, keyed by entry id. */
const cache = new Map();

/**
 * @param {import('./catalog.js').CatalogEntry} entry
 * @returns {Promise<Uint8Array>}
 */
export async function getContent(entry) {
  const cached = cache.get(entry.id);
  if (cached) return cached;

  if (entry.source === 'imported') {
    const bytes = await getRomBytes(entry.id);
    if (!bytes) {
      throw new Error(
        `"${entry.title}" is in the library but its data is missing from storage; re-import the file`,
      );
    }
    // Deliberately not cached: imported ROMs can be large and IndexedDB is fast
    // enough, so holding every launched ROM in memory would only grow the heap.
    return bytes;
  }

  if (entry.source === 'builtin') {
    const response = await fetch(entry.url, { cache: 'force-cache' });
    if (!response.ok) {
      throw new Error(`could not load ${entry.url}: ${response.status} ${response.statusText}`);
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    cache.set(entry.id, bytes);
    return bytes;
  }

  const bytes = synthesise(entry.id, PLACEHOLDER_BYTES);
  cache.set(entry.id, bytes);
  return bytes;
}

/** Whether this entry has content a real core could actually run. */
export function isPlayable(entry) {
  return Boolean(entry?.real);
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

/**
 * Deterministic pseudo-random bytes for a synthetic entry, derived from its id so the
 * same placeholder always produces the same buffer.
 */
function synthesise(seedText, length) {
  let state = 0x811c9dc5;
  for (let i = 0; i < seedText.length; i++) {
    state ^= seedText.charCodeAt(i);
    state = (state * 0x01000193) >>> 0;
  }
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i++) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    bytes[i] = state & 0xff;
  }
  return bytes;
}
