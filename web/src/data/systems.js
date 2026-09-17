/**
 * System catalogue: the consoles the front end knows about.
 *
 * This is UI metadata only — display names, badge glyphs, palette hues, and which
 * core id is expected to run the system. The authoritative technical values
 * (framebuffer geometry, refresh rate, sample rate, pixel format) live in
 * `cores/manifest.json` and are handed to Rust as core declarations. Keeping them
 * apart matters: two cores for the same system can disagree about geometry, and
 * the core must win.
 *
 * `phase: 2` systems are listed but not launchable in the PWA. They are the
 * reason the architecture is what it is — they need JIT, which the browser will
 * not give us, so they wait for the sideloaded iOS build.
 */

export const SYSTEMS = [
  { id: 'nes',     name: 'Nintendo Entertainment System', short: 'NES',  glyph: 'N',  core: 'nestopia',      hue: 355, year: 1983, phase: 1 },
  { id: 'snes',    name: 'Super Nintendo',                short: 'SNES', glyph: 'S',  core: 'snes9x',        hue: 268, year: 1990, phase: 1 },
  { id: 'n64',     name: 'Nintendo 64',                   short: 'N64',  glyph: '64', core: 'mupen64plus',   hue: 142, year: 1996, phase: 1 },
  { id: 'gb',      name: 'Game Boy',                      short: 'GB',   glyph: 'G',  core: 'gambatte',      hue: 88,  year: 1989, phase: 1 },
  { id: 'gbc',     name: 'Game Boy Color',                short: 'GBC',  glyph: 'C',  core: 'gambatte',      hue: 45,  year: 1998, phase: 1 },
  { id: 'gba',     name: 'Game Boy Advance',              short: 'GBA',  glyph: 'A',  core: 'mgba',          hue: 200, year: 2001, phase: 1 },
  { id: 'sms',     name: 'Sega Master System',            short: 'SMS',  glyph: 'M',  core: 'genesis_plus',  hue: 18,  year: 1985, phase: 1 },
  { id: 'genesis', name: 'Sega Genesis / Mega Drive',     short: 'MD',   glyph: 'D',  core: 'genesis_plus',  hue: 220, year: 1988, phase: 1 },
  { id: 'saturn',  name: 'Sega Saturn',                   short: 'SAT',  glyph: 'T',  core: 'yabause',       hue: 300, year: 1994, phase: 1 },
  { id: 'ps1',     name: 'PlayStation',                   short: 'PS1',  glyph: 'P',  core: 'mednafen_psx',  hue: 240, year: 1994, phase: 1 },

  // ---- Phase 2 only: heavier consoles needing sideloaded JIT on iOS. ----
  { id: '3ds',     name: 'Nintendo 3DS',                  short: '3DS',  glyph: '3',  core: 'citra',         hue: 190, year: 2011, phase: 2 },
  { id: 'switch',  name: 'Nintendo Switch',               short: 'NSW',  glyph: 'W',  core: 'yuzu',          hue: 8,   year: 2017, phase: 2 },
];

const BY_ID = new Map(SYSTEMS.map((s) => [s.id, s]));

export function getSystem(id) {
  return BY_ID.get(id) ?? null;
}

export function systemName(id) {
  return BY_ID.get(id)?.name ?? id;
}

/** Systems playable in the browser build. */
export function phase1Systems() {
  return SYSTEMS.filter((s) => s.phase === 1);
}
