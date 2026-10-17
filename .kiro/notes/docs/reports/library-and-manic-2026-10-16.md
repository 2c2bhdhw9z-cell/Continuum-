# Covers and the library (L1 to L9), 16 to 17 October 2026

Built, untested: no phone has seen any of it. In build 191. The owner's phone checks are
TESTING.md "Build 191". Everything is Continuum's own design and wording; Manic only told us what
players expect.

## What changed

- **Whole covers (L1).** `CoverArtView` has a `style`: `.whole` draws the entire cover at the
  picture's own shape (clamped 0.45 to 2.2), never cropped; before a picture arrives, and for a
  game with none, the plate is drawn at the system's usual box shape (`GameSystem.coverAspect` in
  `LibraryExtras.swift`). Shelves: one height (the old card height), each cover as wide as its
  shape. Grids: the old 3:4 space, cover whole and bottom-aligned so titles line up; a console's
  own section uses that console's shape. The system badge is off the art (it's in the line under
  a grid card, and at Extra small that line is the short system name). The hero: the cover
  cropped and blurred as the backdrop, the whole cover sharp in the space above the title; the
  hero is taller (340 to 520 points, was 300 to 430). The list rows and the game card show whole
  covers too. Decoding was already full size with high-quality scaling; the softness came from
  the crop blowing covers up.
- **More covers found (L1).** The listing search (`ArtworkIndex.search`) also matches titles whose
  words break differently ("Leaf Green Version" / "LeafGreen Version": `squashEqualTitles`,
  `squashAligns`, same guards as before) and retries once without a numbered set's catalogue
  number in front ("1617 - Pokemon Leaf Green"). The server's GBA list has "Pokemon - LeafGreen
  Version (USA)"; the GoodTools spelling now finds it.
- **All Games by console (L2).** A third layout, **By console**, now the default: a section per
  console (name, game count, a chevron that folds it; folds remembered), pinned headers, an index
  of short console names down the right edge (tap or slide), Random Game (shuffle button), select
  mode (tick-circle button: All/None, star, delete with a confirmation), and Settings, LAYOUT,
  **Console order...** (drag list; Home's per-system shelves follow it too). `ConsoleSections`,
  `LibraryPrefs`, `LibrarySelection`, `ConsoleOrderSheet`.
- **Patch this game... (L3).** Our own IPS, BPS and UPS (`crates/emulator-bridge/src/import/patch.rs`,
  6 tests): BPS/UPS checked by their CRC32s (a patch for another version is refused in one line),
  UPS both ways, a SNES copier header tried without when the checksum says so, IPS RLE and the
  size cut. Always a new file, "<name> [<patch>].<ext>", next to the original; never overwrites.
  Not offered for zips, playlists and disc sheets.
- **Join it with its other discs... (L4).** On a disc of PS1, Saturn, Sega CD, PC Engine CD or
  Dreamcast: tick the discs (this game's are pre-ticked), name it, and Continuum writes
  "<name>.m3u". The Library now leaves out every disc an .m3u names, so a multi-disc game shows
  once (`MultiDisc.namedByPlaylists` in `refreshLibrary`).
- **Copy its link (L5).** `continuum://play?game=<file name>`; the URL scheme is in project.yml
  (and the seed Info.plist); `ImportCenter.open` hands it to `EngineHost.openLaunchLink`, which
  waits 0.7 s on a cold start, finds the game and starts it.
- **Share the game (L6).** The share sheet with the file, a .cue's tracks, a .gdi's tracks, or a
  playlist's discs and their tracks.
- **Clean start (L7).** One start with no cheats, no auto-load and none of the game's own core
  settings (engine: `options::set_skip_game_choices_next_start`, used once; the game then runs
  as "no game", so nothing can write over its own settings file). The auto-save is held for the
  session, so leaving doesn't overwrite it. 2 engine tests.
- **Console language for this game (L8).** On the game card, when the game's emulator has a
  language setting (found by name in its own option table: `options::language_definition`). It
  shows once that emulator has run on the phone, because that's how Continuum learns its
  settings. Written to the game's own settings file without the game running
  (`options::set_game_value`). 2 engine tests.
- **Mute games with the silent switch (L9).** Settings, SOUND. On: the audio category is
  `.ambient`, which follows the switch (and mixes with other apps' sound); off (the default):
  `.playback`, as before.

## Checked here

Workspace tests (747 in the engine plus the rest), clippy clean, every changed Swift file parses,
the new engine calls typecheck against freshly generated bindings, and LibraryExtras.swift is in
project.yml. Swift's own type check only happens on the build server.

## Not done or not known

- Covers come from the same libretro server. Manic may use bigger pictures; ours are drawn at
  full size, never shrunk.
- The language menu needs one run of the emulator first. Which emulators have a language setting
  is read from each one's own table; untested which ones the phone shows it for.
- Select mode is in All Games' By console and Grid layouts, not List (List has swipe to delete).
- Emulator updates found at the start of this session and held for the next batch: Gearsystem,
  Azahar, PPSSPP and Flycast have newer code upstream; Amiga (PUAE) and C64 (VICE) have newer
  buildbot builds (`docs/CORE_UPDATES.md`).
