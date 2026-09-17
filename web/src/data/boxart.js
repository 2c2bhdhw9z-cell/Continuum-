/**
 * Cover art resolution against the libretro thumbnail server.
 *
 * ## Why this stores a URL and not the image
 *
 * `thumbnails.libretro.com` serves no `Access-Control-Allow-Origin` header. Verified,
 * not assumed — a `GET` with an `Origin` returns `200` with no CORS header at all, and
 * `OPTIONS` answers without one either. The consequences are absolute:
 *
 *   - `fetch()` in `cors` mode is refused, so the bytes cannot be read.
 *   - `no-cors` mode yields an opaque response whose `status` is always `0`, so it
 *     cannot even be used to tell a hit from a miss.
 *   - Drawing the image into a canvas taints it, so `toBlob`/`getImageData` throw.
 *
 * What *does* work cross-origin is an `<img>`. So the ladder is probed with `Image`
 * objects — `onload` means the file exists, `onerror` means it does not — and what
 * gets persisted is the resolved address. The image bytes then live in the browser's
 * HTTP cache, which is also the honest limit of this tier: scraped art is not
 * guaranteed offline. Artwork that *is* guaranteed offline comes from the two tiers
 * that produce real blobs — an in-game capture, or a file the user picked.
 *
 * A 404 from this server returns `text/html`, so `onerror` fires reliably rather than
 * an error page being decoded as a broken image.
 *
 * ## The ladder
 *
 * ```text
 *   1. Named_Boxarts/{name}.png     the actual cover
 *   2. Named_Titles/{name}.png      title screen
 *   3. Named_Snaps/{name}.png       in-game shot
 *   4-6. the same three, with [..] and (..) tags stripped
 * ```
 *
 * Tags are stripped only as a fallback because they are load-bearing in the libretro
 * database: "Sonic (USA)" and "Sonic (Japan)" are different files with different
 * covers, so dropping the tag first would confidently fetch the wrong region's art.
 *
 * ## Privacy
 *
 * Resolving art sends a ROM's filename to a third-party server. That is a real
 * disclosure, not an implementation detail, so it is a setting the user can turn off
 * and the Settings sheet says what it does. Requests carry no referrer.
 */

import { putArt } from './rom-store.js';

const THUMBNAIL_HOST = 'https://thumbnails.libretro.com';

/**
 * Our system ids to libretro playlist directory names.
 *
 * Every one of these was checked against the live server rather than transcribed from
 * memory — the names are not guessable ("Sega - Master System - Mark III", not
 * "Sega - Master System"), and a wrong directory is indistinguishable from a game
 * having no art.
 *
 * Phase 2 systems are absent deliberately: they cannot be imported, so they can never
 * reach this code.
 */
export const LIBRETRO_DIRS = {
  nes: 'Nintendo - Nintendo Entertainment System',
  snes: 'Nintendo - Super Nintendo Entertainment System',
  n64: 'Nintendo - Nintendo 64',
  gb: 'Nintendo - Game Boy',
  gbc: 'Nintendo - Game Boy Color',
  gba: 'Nintendo - Game Boy Advance',
  sms: 'Sega - Master System - Mark III',
  genesis: 'Sega - Mega Drive - Genesis',
  saturn: 'Sega - Saturn',
  ps1: 'Sony - PlayStation',
};

/** The thumbnail folders, in the order the specification asks for. */
const FOLDERS = [
  ['Named_Boxarts', 'boxart'],
  ['Named_Titles', 'title'],
  ['Named_Snaps', 'snap'],
];

/**
 * Characters libretro requires to be replaced with `_` in a thumbnail filename.
 *
 * The set, one per line so that the list cannot accidentally close this comment —
 * spelling it inline is how the sequence asterisk-slash ends up inside a block
 * comment and truncates the file:
 *
 *   ampersand, asterisk, forward slash, colon, backtick,
 *   less-than, greater-than, question mark, backslash, pipe, double quote
 *
 * This is a *substitution*, not a strip: the underscore holds the character's
 * position, so "Ratchet & Clank" becomes "Ratchet _ Clank" with both spaces intact.
 * Removing the character instead would produce "Ratchet  Clank" and never match.
 */
const INVALID = /[&*/:`<>?\\|"]/g;

export function sanitizeForLibretro(name) {
  return name.replace(INVALID, '_');
}

/** "Super Mario World (USA) [!].sfc" → "Super Mario World (USA) [!]". */
export function baseName(filename) {
  return filename.replace(/\.[^.]+$/, '');
}

function collapse(name) {
  return name.replace(/\s{2,}/g, ' ').trim();
}

/**
 * Drops only the square-bracket tags: `[!]`, `[b1]`, `[T+Eng]` and the rest of the
 * GoodTools dump vocabulary.
 *
 * This is the step that earns its place. Libretro names games after the No-Intro
 * database, which uses parenthesised tags — "Super Mario World (USA)" — while dump
 * tags in square brackets are a different convention layered on top. So stripping
 * *everything* on the first retry throws away the region along with the noise:
 * "Super Mario World (USA) [!]" becomes "Super Mario World", which does not exist on
 * the server, when "Super Mario World (USA)" does. Measured, not assumed — the two
 * URLs return 404 and 200 respectively.
 */
export function stripDumpTags(name) {
  return collapse(name.replace(/\[[^\]]*\]/g, ' '));
}

/** Drops every `[...]` and `(...)` tag. The last resort. */
export function stripTags(name) {
  return collapse(name.replace(/[([][^)\]]*[)\]]/g, ' '));
}

/**
 * The ordered candidate list for a ROM: three name forms, each across three folders,
 * from most specific to least.
 *
 * Name-major rather than folder-major, because an exact-name box art is by far the
 * most common outcome — so the common case costs one request, and only a genuine miss
 * walks the whole ladder. A total miss is then remembered for a week.
 *
 * @param {string} systemId
 * @param {string} filename
 * @returns {{url: string, tier: string, name: string}[]}
 */
export function candidates(systemId, filename) {
  const dir = LIBRETRO_DIRS[systemId];
  if (!dir) return [];

  const base = baseName(filename);
  const forms = [
    { name: sanitizeForLibretro(collapse(base)), suffix: '' },
    { name: sanitizeForLibretro(stripDumpTags(base)), suffix: '-relaxed' },
    { name: sanitizeForLibretro(stripTags(base)), suffix: '-untagged' },
  ];

  const out = [];
  const seen = new Set();
  for (const form of forms) {
    // A form that reduced to nothing, or to something already tried, is skipped —
    // "Tetris (World)" has no square-bracket tags, so the relaxed form is identical to
    // the exact one and probing it again would be three wasted requests.
    if (!form.name || seen.has(form.name)) continue;
    seen.add(form.name);
    for (const [folder, tier] of FOLDERS) {
      out.push({
        url: `${THUMBNAIL_HOST}/${encodeURIComponent(dir)}/${folder}/${encodeURIComponent(form.name)}.png`,
        tier: `${tier}${form.suffix}`,
        name: form.name,
      });
    }
  }
  return out;
}

/** How long a probe may hang before being treated as a miss. */
const PROBE_TIMEOUT_MS = 8000;

/**
 * Does this URL resolve to a decodable image?
 *
 * @param {string} url
 * @returns {Promise<boolean>}
 */
export function probe(url) {
  return new Promise((resolve) => {
    if (typeof Image === 'undefined') {
      resolve(false);
      return;
    }
    const image = new Image();
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      image.onload = null;
      image.onerror = null;
      // Abort an in-flight request: leaving several megabytes downloading for art
      // nobody is waiting for is wasteful on a phone.
      if (!result) image.src = '';
      resolve(result);
    };
    const timer = setTimeout(() => finish(false), PROBE_TIMEOUT_MS);

    // No referrer: the third party does not need to know which page asked.
    image.referrerPolicy = 'no-referrer';
    image.decoding = 'async';
    image.onload = () => finish(image.naturalWidth > 0 && image.naturalHeight > 0);
    image.onerror = () => finish(false);
    image.src = url;
  });
}

// ------------------------------------------------------------- negative caching

const MISS_KEY = 'continuum:boxart-misses:v1';
/** A miss is worth retrying eventually: the repository gains thumbnails over time. */
const MISS_TTL_MS = 7 * 24 * 60 * 60 * 1000;

function readMisses() {
  try {
    const raw = localStorage.getItem(MISS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

function writeMisses(misses) {
  try {
    localStorage.setItem(MISS_KEY, JSON.stringify(misses));
  } catch {
    // Storage refused (private browsing, quota). The only cost is re-probing next
    // boot, which is not worth failing an import over.
  }
}

/** Whether we recently established that this entry has no art on the server. */
export function isKnownMiss(id) {
  const at = readMisses()[id];
  return typeof at === 'number' && Date.now() - at < MISS_TTL_MS;
}

function recordMiss(id) {
  const misses = readMisses();
  misses[id] = Date.now();
  writeMisses(misses);
}

export function clearMiss(id) {
  const misses = readMisses();
  if (id in misses) {
    delete misses[id];
    writeMisses(misses);
  }
}

export function clearAllMisses() {
  writeMisses({});
}

// --------------------------------------------------------------------- fetching

/**
 * Probes run through a small queue.
 *
 * Importing a folder of thirty ROMs would otherwise open up to 180 connections at
 * once: the browser would serialise them anyway, but the phone would spend its
 * radio and its memory on artwork while the user is waiting to play something.
 * Three at a time keeps the network available for the core and the ROM.
 */
const MAX_CONCURRENT = 3;
let active = 0;
const waiting = [];

function acquire() {
  if (active < MAX_CONCURRENT) {
    active++;
    return Promise.resolve();
  }
  return new Promise((resolve) => waiting.push(resolve));
}

function release() {
  const next = waiting.shift();
  if (next) next();
  else active--;
}

/**
 * Walks the ladder for one entry and persists the first hit.
 *
 * Built-in carts are never probed: they are original ROMs written for this project,
 * so no thumbnail server has heard of them, and asking would be six guaranteed 404s
 * per cart on every fresh install.
 *
 * @param {{id: string, systemId: string, filename: string, source: string}} entry
 * @returns {Promise<{kind: 'url', url: string, tier: string}|null>}
 */
export async function resolveArtwork(entry) {
  if (!entry || entry.source === 'builtin') return null;
  if (!LIBRETRO_DIRS[entry.systemId]) return null;
  if (isKnownMiss(entry.id)) return null;

  const list = candidates(entry.systemId, entry.filename);
  if (!list.length) return null;

  await acquire();
  try {
    for (const candidate of list) {
      if (await probe(candidate.url)) {
        const art = { kind: 'url', url: candidate.url, tier: candidate.tier };
        try {
          await putArt({ id: entry.id, kind: 'url', url: candidate.url, tier: candidate.tier });
        } catch (err) {
          console.warn('[boxart] could not persist art for', entry.id, err);
        }
        clearMiss(entry.id);
        return art;
      }
    }
  } finally {
    release();
  }

  recordMiss(entry.id);
  return null;
}
