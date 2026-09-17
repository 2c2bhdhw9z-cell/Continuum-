/**
 * ROM/disc content provider.
 *
 * Phase 1 has no real ROMs to ship — nobody can legally bundle them — so this
 * synthesises deterministic placeholder content. The placeholder deliberately
 * satisfies the same contract the real store will: async, returns a `Uint8Array`,
 * may reject. Every caller is therefore already written against the real shape.
 *
 * TODO(phase1b): replace `getContent` with an IndexedDB (or OPFS, for the multi-GB
 * disc images Phase 2 needs) lookup, plus a file-import flow:
 *
 *   1. `showOpenFilePicker` / `<input type=file>` → `File` handles;
 *   2. hash the header to identify the system and pick a core;
 *   3. store the blob under its content id, keep metadata in the catalogue;
 *   4. `getContent` streams it back out.
 *
 * Nothing above this module changes when that lands.
 */

/** Placeholder payload size. Small — it exists to be non-empty, not to be read. */
const PLACEHOLDER_BYTES = 64 * 1024;

const cache = new Map();

/**
 * @param {{ id: string, title: string }} entry
 * @returns {Promise<Uint8Array>}
 */
export async function getContent(entry) {
  const cached = cache.get(entry.id);
  if (cached) return cached;

  const bytes = synthesise(entry.id, PLACEHOLDER_BYTES);
  cache.set(entry.id, bytes);
  return bytes;
}

/** True once real content exists for this entry. Always false in Phase 1. */
export function hasRealContent(_entry) {
  return false;
}

/**
 * Deterministic pseudo-random bytes derived from the content id, so the same game
 * always produces the same "ROM" and any content-dependent bug is reproducible.
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
