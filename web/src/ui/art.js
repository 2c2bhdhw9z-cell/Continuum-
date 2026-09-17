/**
 * Procedural box art.
 *
 * There is no artwork to ship and no CDN to fetch it from, but a wall of grey
 * rectangles makes the library impossible to evaluate. So art is generated from
 * the title string: a stable hash picks a hue pair and a pattern, and the result
 * is a CSS gradient. Zero requests, zero bytes, no layout shift, and it works
 * offline — which matters for a PWA whose whole point is running without a network.
 *
 * TODO(phase1b): when real cover art exists, swap `artFor` for a lazily fetched
 * image with this gradient as the placeholder. The virtualiser needs the art box to
 * keep a fixed size, so the image must be `object-fit: cover` inside it — never
 * allowed to influence layout.
 */

import { getSystem } from '../data/systems.js';

/** FNV-1a. Cheap, and stable across engines so art never changes between reloads. */
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
  (a, b) =>
    `radial-gradient(120% 90% at 30% 20%, hsl(${a} 78% 56%), hsl(${b} 62% 12%))`,
  // Hard-edged retro bands.
  (a, b) =>
    `linear-gradient(160deg, hsl(${a} 68% 48%) 0 38%, hsl(${b} 64% 30%) 38% 62%, hsl(${b} 60% 12%) 62%)`,
  // Corner sweep.
  (a, b) =>
    `conic-gradient(from 200deg at 70% 30%, hsl(${a} 72% 50%), hsl(${b} 58% 16%), hsl(${a} 60% 28%))`,
];

/**
 * CSS `background` value for an entry.
 * The system's hue anchors the palette, so every NES game reads as part of a set
 * while individual titles stay distinguishable.
 */
export function artFor(entry) {
  const h = hash(entry.title + entry.id);
  const system = getSystem(entry.systemId);
  const baseHue = system ? system.hue : h % 360;
  const hueA = (baseHue + (h % 40) - 20 + 360) % 360;
  const hueB = (hueA + 150 + (h >>> 8) % 60) % 360;
  const pattern = PATTERNS[(h >>> 16) % PATTERNS.length];
  return pattern(hueA, hueB);
}

/** Two-character glyph for the art plate: system short code or title initials. */
export function glyphFor(entry) {
  const system = getSystem(entry.systemId);
  if (system) return system.glyph;
  const words = entry.title.split(/\s+/);
  return (words[0]?.[0] ?? '?') + (words[1]?.[0] ?? '');
}

/** Compact metadata line: "SNES · 1994 · JRPG". */
export function subtitleFor(entry) {
  const system = getSystem(entry.systemId);
  return `${system?.short ?? entry.systemId} · ${entry.year} · ${entry.genre}`;
}

/** Long metadata line for the hero and detail sheet. */
export function metaLineFor(entry) {
  const system = getSystem(entry.systemId);
  const size =
    entry.sizeMb >= 1
      ? `${entry.sizeMb.toFixed(1)} MB`
      : `${Math.round(entry.sizeMb * 1024)} KB`;
  return [
    system?.short ?? entry.systemId,
    entry.year,
    entry.region,
    `${entry.rating.toFixed(1)}/10`,
    `${entry.players}P`,
    size,
  ].join('  ·  ');
}
