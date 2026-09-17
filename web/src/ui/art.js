/**
 * The procedural console plate: tier 5 of the cover-art ladder, and the reason no
 * card is ever blank.
 *
 * When no artwork has been resolved — nothing on the libretro server, nothing captured
 * yet, nothing the user picked — the card still has to look deliberate. So a plate is
 * generated from the title: a stable hash picks a pattern, the *system* anchors the
 * hue, and the result is a CSS gradient. Zero requests, zero bytes, no layout shift,
 * and it works with no network at all.
 *
 * Anchoring on the system is what makes it read as console-themed rather than random:
 * every Game Boy plate lands in the same green family, every GBA plate in the same
 * blue, so a shelf looks like a set while individual titles stay distinguishable.
 *
 * Real artwork, when there is any, is drawn *over* this by `card.js` — so the plate is
 * also the loading state for a cover that has not decoded yet, and the letterbox
 * behind one that does not fill its box.
 */

import { getSystem } from '../data/systems.js';
import { formatBytes } from '../data/catalog.js';

/** FNV-1a. Cheap, and stable across engines so a plate never changes between reloads. */
function hash(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = (h * 0x01000193) >>> 0;
  }
  return h;
}

const PATTERNS = [
  // Diagonal duotone.
  (a, b) => `linear-gradient(135deg, hsl(${a} 62% 42%), hsl(${b} 58% 14%))`,
  // Vertical fade with a bright horizon.
  (a, b) =>
    `linear-gradient(180deg, hsl(${a} 70% 52%) 0%, hsl(${b} 60% 22%) 55%, hsl(${b} 55% 10%) 100%)`,
  // Radial spotlight.
  (a, b) => `radial-gradient(120% 90% at 30% 20%, hsl(${a} 78% 56%), hsl(${b} 62% 12%))`,
  // Hard-edged retro bands.
  (a, b) =>
    `linear-gradient(160deg, hsl(${a} 68% 48%) 0 38%, hsl(${b} 64% 30%) 38% 62%, hsl(${b} 60% 12%) 62%)`,
  // Corner sweep.
  (a, b) =>
    `conic-gradient(from 200deg at 70% 30%, hsl(${a} 72% 50%), hsl(${b} 58% 16%), hsl(${a} 60% 28%))`,
];

/** CSS `background` value for an entry's fallback plate. */
export function artFor(entry) {
  const h = hash(entry.title + entry.id);
  const system = getSystem(entry.systemId);
  const baseHue = system ? system.hue : h % 360;
  const hueA = (baseHue + (h % 40) - 20 + 360) % 360;
  const hueB = (hueA + 150 + ((h >>> 8) % 60)) % 360;
  const pattern = PATTERNS[(h >>> 16) % PATTERNS.length];
  return pattern(hueA, hueB);
}

/** Two-character glyph for the plate: system short code or title initials. */
export function glyphFor(entry) {
  const system = getSystem(entry.systemId);
  if (system) return system.glyph;
  const words = entry.title.split(/\s+/);
  return (words[0]?.[0] ?? '?') + (words[1]?.[0] ?? '');
}

/**
 * Compact metadata line for a card: "SNES · 4.0 MB" or "SNES · USA · 4.0 MB".
 *
 * Only what is known. There is no year, genre or rating here because nothing in a ROM
 * file reliably carries them, and a front end that prints a confident "1994 · JRPG ·
 * 8.4/10" under a file it was handed five seconds ago is making it up.
 */
export function subtitleFor(entry) {
  const system = getSystem(entry.systemId);
  return [system?.short ?? entry.systemId, entry.region, formatBytes(entry.sizeBytes)]
    .filter(Boolean)
    .join('  ·  ');
}

/** Longer metadata line for the hero and the detail sheet. */
export function metaLineFor(entry) {
  const system = getSystem(entry.systemId);
  const parts = [system?.short ?? entry.systemId];
  if (entry.region) parts.push(entry.region);
  parts.push(formatBytes(entry.sizeBytes));
  parts.push(entry.source === 'builtin' ? 'Bundled cart' : 'Imported');
  if (entry.playCount > 0) {
    parts.push(entry.playCount === 1 ? 'played once' : `played ${entry.playCount} times`);
  }
  return parts.join('  ·  ');
}
