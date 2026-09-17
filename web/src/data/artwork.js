/**
 * Cover art, across all five tiers, and the one place that knows how to display it.
 *
 * ```text
 *   1  libretro Named_Boxarts        boxart.js            stored as a URL
 *   2  libretro Named_Titles/Snaps   boxart.js            stored as a URL
 *   3  the same, tags stripped       boxart.js            stored as a URL
 *   4  in-game capture past frame 60 player-view + png.js stored as a blob
 *   5  a file the user picked        settings/detail sheet stored as a blob
 * ```
 *
 * Tiers 1–3 can only be a URL, because the thumbnail server sends no CORS headers
 * (see `boxart.js`). Tiers 4 and 5 are ours, so they are real blobs and work offline.
 * Everything with no art at all falls through to the procedural console-themed plate
 * in `ui/art.js`, which is why no card is ever blank.
 *
 * ## Object URL ownership
 *
 * A blob needs an object URL to be displayed, and an object URL is a leak until it is
 * revoked. Cards are recycled constantly, so minting one per bind would leak a URL per
 * scroll frame — the exact class of bug the virtualiser exists to avoid. So each entry
 * gets at most one URL, created when its blob is attached and revoked when the blob is
 * replaced or the entry is deleted. Binding a card is then a property read.
 */

import { entryById, setArtwork } from './catalog.js';
import { listArt, putArt, deleteArt } from './rom-store.js';
import { resolveArtwork, clearMiss } from './boxart.js';
import { encodePng } from './png.js';

/** entryId -> object URL, for blob-backed art only. */
const objectUrls = new Map();

/** Frames a game must survive before its output is worth keeping as a thumbnail. */
export const CAPTURE_AFTER_FRAME = 60;

/**
 * Thumbnail capture size.
 *
 * 4:3 at 512 wide: the renderer will scale the core's framebuffer into exactly these
 * dimensions, so nothing is resampled in JavaScript, and 512 matches the width
 * libretro uses for its own thumbnails. Cards crop to their poster shape with
 * `object-fit: cover`.
 */
export const CAPTURE_WIDTH = 512;
export const CAPTURE_HEIGHT = 384;

function releaseUrl(id) {
  const url = objectUrls.get(id);
  if (url) {
    URL.revokeObjectURL(url);
    objectUrls.delete(id);
  }
}

function attach(id, record) {
  releaseUrl(id);
  if (!record) {
    setArtwork(id, null);
    return null;
  }
  if (record.kind === 'blob' && record.blob) {
    const url = URL.createObjectURL(record.blob);
    objectUrls.set(id, url);
    setArtwork(id, { kind: 'blob', url, tier: record.tier, bytes: record.blob.size });
    return url;
  }
  if (record.kind === 'url' && record.url) {
    setArtwork(id, { kind: 'url', url: record.url, tier: record.tier });
    return record.url;
  }
  return null;
}

/**
 * Loads every stored art record and attaches it to the matching library entry.
 *
 * Called after the library has been populated from storage, so it is one `getAll()`
 * rather than a lookup per entry.
 *
 * @returns {Promise<number>} how many entries gained artwork
 */
export async function hydrateArtwork() {
  let applied = 0;
  try {
    for (const record of await listArt()) {
      if (!entryById(record.id)) continue;
      if (attach(record.id, record)) applied++;
    }
  } catch (err) {
    console.warn('[artwork] could not restore artwork', err);
  }
  return applied;
}

/** A displayable URL for this entry, or null to use the procedural plate. */
export function displayUrlFor(entry) {
  return entry?.art?.url ?? null;
}

/**
 * Tiers 1–3 for one entry. Safe to call for anything: built-ins, unsupported systems
 * and recent misses all return null without touching the network.
 */
export async function scrapeArtwork(entry) {
  if (!entry || entry.art) return null;
  const art = await resolveArtwork(entry);
  if (!art) return null;
  attach(entry.id, art);
  return art;
}

/**
 * Tier 4: keep what the game is actually drawing.
 *
 * @param {string} entryId
 * @param {Uint8Array} rgba tightly packed, `CAPTURE_WIDTH * CAPTURE_HEIGHT * 4`
 * @param {number} width
 * @param {number} height
 */
export async function storeCapture(entryId, rgba, width, height) {
  const blob = await encodePng(rgba, width, height);
  const record = { id: entryId, kind: 'blob', blob, tier: 'capture' };
  await putArt(record);
  attach(entryId, record);
  return record;
}

/**
 * Tier 5: a file the user chose. Overwrites whatever any earlier tier resolved, and
 * clears the negative cache so removing it later can scrape again.
 *
 * @param {string} entryId
 * @param {File|Blob} file
 */
export async function storeManualArtwork(entryId, file) {
  if (!file) throw new Error('no file was selected');
  if (!file.type?.startsWith('image/')) {
    throw new Error(`${file.type || 'that file'} is not an image`);
  }
  // 12 MB is far more than a cover needs and still leaves room for a photo taken on
  // the device; beyond that the library becomes the thing filling the quota.
  if (file.size > 12 * 1024 * 1024) {
    throw new Error(`${(file.size / 1048576).toFixed(1)} MB is too large for cover art`);
  }
  // Stored as its own blob rather than re-encoded: the bytes the user picked are
  // already a valid image, and re-encoding would need a decode step that buys nothing.
  const record = { id: entryId, kind: 'blob', blob: file, tier: 'manual' };
  await putArt(record);
  clearMiss(entryId);
  attach(entryId, record);
  return record;
}

/** Forgets an entry's artwork, so it falls back to the procedural plate. */
export async function clearArtwork(entryId) {
  await deleteArt(entryId);
  clearMiss(entryId);
  releaseUrl(entryId);
  setArtwork(entryId, null);
}

/** Drops every object URL. For "clear all storage", which empties the store too. */
export function releaseAllUrls() {
  for (const id of [...objectUrls.keys()]) releaseUrl(id);
}

/** Human-readable provenance for the detail sheet. */
export function describeTier(art) {
  if (!art) return 'No cover art — showing the generated console plate.';
  switch (art.tier) {
    case 'boxart':
      return 'Box art from the libretro thumbnail archive.';
    case 'title':
      return 'Title screen from the libretro thumbnail archive.';
    case 'snap':
      return 'In-game shot from the libretro thumbnail archive.';
    case 'boxart-relaxed':
    case 'title-relaxed':
    case 'snap-relaxed':
      return 'From the libretro archive, matched after ignoring the dump tags in the filename.';
    case 'boxart-untagged':
    case 'title-untagged':
    case 'snap-untagged':
      return 'From the libretro archive, matched after dropping every filename tag — it may be another region or revision.';
    case 'capture':
      return 'Captured from this game while you played it.';
    case 'manual':
      return 'The image you chose.';
    default:
      return 'Cover art.';
  }
}
