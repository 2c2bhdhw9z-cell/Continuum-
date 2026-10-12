# Idea: play the gen1recomp games (and the voxel 3D mod) inside Continuum

The owner raised this on 12 October 2026. **Nothing is decided or built yet.** These notes come
from one look at the project's licence, releases and mods the same day.

## What it is
- **gen1recomp++ / "G1R Deluxe"** (github.com/bryanthaboi/gen1recomp, by BOIS CLUB GAMES, LLC)
  is not an emulator. It is the Pokémon games rewritten by hand in Lua on the LÖVE game engine.
  Game data and graphics are decoded from a ROM the player supplies, and no ROM bytes ship.
  Red, Blue and Yellow are the main games; Gold to Emerald are newer, some marked beta. It has
  TILT, ZOOM, SPEED and COLOR options, mods, online play, boxes and skins.
- Releases come out very often: 0.3.82 and 0.3.83 were both published on 12 October. There is
  an official iOS `.ipa` (about 56 MB) and a plain `.love` game file (about 43 MB).
- The **voxel 3D look** is a separate mod: Dramatic Shape Voxel Mod and its forks (for example
  DRAMALESS_SHAPE, and a Gen 2 and Emerald fork).

## Licence (`LICENSE.MD` in that repo; general reading, not legal advice)
- The game code is **GPL v3, with extra terms**. GPL v3 is the same licence as Continuum, so the
  game code may be used.
- **Extra term 1:** a visible credit is required in the README, in any in-app credits or about
  screen, AND inside any launcher or menu used to start the game. It must say at least:
  "Based on the Pokemon Gen 1 Recompilation Project by BOIS CLUB GAMES, LLC
  (https://github.com/bryanthaboi/gen1recomp)". This cannot be skipped.
- **Extra term 2:** their launcher (`src/import/LauncherView.lua`, `LauncherSettings.lua`,
  `OnlinePanel.lua`, `CartLabelArt.lua`, `CartShape.lua`, `src/mods/LauncherMods.lua`, the
  launcher UI parts of `RomImporter.lua`, `assets/launcher/`, `assets/labels/`) is proprietary.
  It can't be copied or reused. Continuum would have to make its own launcher (its own tab),
  and that launcher must carry the credit. Their online panel, cart art and mod browser are part
  of that launcher, so Continuum would need its own versions or would go without them.
- **Extra term 3:** no use of their names or logos beyond the credit. So no "G1R Deluxe"
  branding.
- **Voxel mod:** at least the original Dramatic Shape mod forbids redistribution after its v1.
  So Continuum can't bundle it. Players could add mods themselves from each author's own page,
  the way G1R's mod index works (it lists where each mod lives and holds no mod code).

## Technical shape (to be investigated before deciding)
- Bundle the LÖVE engine (zlib licence, no limits) plus a pinned version of the GPL game files.
  Add a Continuum tab as the launcher: pick a game, pick a Pokémon ROM that is already in the
  Continuum library (no second import needed), saves, options and a mod installer.
- Open questions:
  - Can LÖVE run as one screen inside Continuum? On iPhone it normally takes over the whole app
    through SDL.
  - Which LÖVE version does G1R use, and does it draw with OpenGL ES or Metal?
  - App size: about +45 to 60 MB.
  - Where saves live, so backup and sync can include them.
  - How mods are installed and kept up to date.
  - Speed: it already runs on the owner's iPhone in the official app.
  - Whether controllers and Continuum's on-screen controls or skins can drive it.
  - Save states, rewind and cheats won't apply; G1R has its own saves and speed option.
- Updates would follow the same rule as emulators: pinned, checked, then taken.
- A quick fallback, if embedding is too hard: a button that opens the separate G1R app if it is
  installed.
- **Risk:** Nintendo often takes down Pokémon fan projects. If G1R is ever taken down, this tab
  would probably have to go as well.
