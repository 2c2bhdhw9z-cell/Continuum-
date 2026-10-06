# Continuum

One app for the iPhone. It plays games from older consoles. You install the `.ipa` yourself (ESign
or a similar installer). It is not on the App Store.

**The install:** always the newest file on the
[Releases page](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest), named
`Continuum-<number>.ipa`. Pushes that change the app build one automatically: quick checks on
Linux first, then about 30 minutes on a Mac. A second push waits for the first instead of
cancelling it. As of 6 October 2026 that is build 127 (release `build-127-a611e43`, about 85 MB)
or newer. On that build the app shows its version as `0.8.0 (127)`.

This install is **not** signed with a JIT. Heavier systems run on an interpreter.

## What is actually true

**Done on a phone**

- NES, SNES, Game Boy Color, Game Boy Advance, Game Gear, Genesis
- Nintendo DS (Mario Kart DS and Pokémon SoulSilver)
- Nintendo 64, software only (Smash reached the character select, about 60 fps). No JIT.
- PlayStation, both cores (Crash, with and without a BIOS)
- Nintendo 3DS: Mario Kart 7 runs (build 122). A stutter after transitions was seen on builds
  100 and 101; a fix is in, not yet confirmed gone
- Rewind, fast forward (about 4x), sound, controllers, auto-save and resume, typed cheats
- The **+** button opens Files, the in-game **⋯** menu, the layout editor, the system pickers
- TV/AirPlay picture (sharper since build 119)
- Landscape with no skin imported (fixed in build 120)
- Confirmed 5 October with build 122: the eye button that hides the player's top bar (and
  remembers), a paused game staying paused after leaving the app, a 3DS skin sideways with both
  screens in their holes and nothing on the picture, importing several skins at once, the tidier
  diagnostic text, changing a 3DS restart-required setting without a crash, fast forward 2x, the
  CRT filter, taking a cover from the running game, and Save to the next free slot
- Confirmed 5 October with build 125: Game Boy, TurboGrafx-16, Atari 2600, Jaguar and Pokemon
  Mini; save and load on 14 systems; save slot pictures; exporting and importing a state file;
  the switch that hides Apple's performance overlay; restarting from Core settings; cover lookups
  off; paste, Open in and zip imports; the sharper cover from the game; battery saves; renaming,
  saving over and deleting slots; the keyboard, tilt and shake; 3x, 4x and slow motion; a
  RetroAchievements unlock banner; haptics and rumble; hiding the pad with a controller; the LCD
  grid and dot matrix filters; palette and rotate; swap screens and the six two-screen layouts;
  the game on a TV with TV scaling and layout
- Two bugs found in build 125 are fixed in build 126 (not on a phone yet): every 3DS save state
  was refused as "too short", and an imported save state had no picture. Build 126 also adds a
  feedback form (Settings, and a game's ⋯ menu) and typing cheat codes inside a game

**In the app, not confirmed on a phone yet**

Everything in [TESTING.md](TESTING.md)'s list, in short: 23 more systems (38 in all: 32 emulator
cores plus the Flash and J2ME players), including PSP, Dreamcast, Saturn, arcade and the
computers; Wi-Fi transfer, skin holes upright and on DS skins (sideways 3DS is confirmed), Manic
skins, extra buttons, cheat search, two-screen
layouts, online play, cloud sync, and the rest of build 124's bug sweep
(listed in STATUS.md).

**Not in the app**

- Switch. Not built. It is a system to emulate, not a device this app runs on.
- GameCube and Wii: not possible without JIT ([docs/HARD_SYSTEMS.md](docs/HARD_SYSTEMS.md)).
- SMB network shares (WebDAV works).
- JIT. Left out on purpose.

The line-by-line table is in [STATUS.md](STATUS.md). [TESTING.md](TESTING.md) is the live test
list with easy steps. What the owner wants built is in
[docs/PRODUCT_SCOPE.md](docs/PRODUCT_SCOPE.md). [docs/MANIC_PARITY.md](docs/MANIC_PARITY.md) is the
checklist for doing everything Manic EMU does. `.kiro/steering/owner-rules.md` holds the owner's
rules. Old notes live in `docs/archive/`.

## How to add games

Tap **+** in the Library: it opens Files. Pick the game files. For a PlayStation, Saturn or other
disc game, pick the `.cue` and every `.bin` in the same go. Hold your finger on **+** for Wi-Fi
transfer, paste and servers. You can also drop files in the Files app under On My iPhone →
Continuum.

A 3DS game has to be a decrypted `.3ds`, `.3dsx`, `.cci`, or `.cxi`. A PSP game can be `.cso`,
`.iso`, `.chd`, a PSP `EBOOT.PBP`, or `.prx`; the app looks inside an `.iso`, `.chd` or `.pbp`, and
if it cannot tell PSP from PlayStation it asks **Which system?** once and remembers. `.elf` is not
accepted. BIOS files (for example `scph1001.bin` for the Beetle PlayStation option) go in that
same Continuum folder; the app names the exact file when one is missing.

## Settings worth knowing

- **Settings → DIAGNOSTICS → Apple performance overlay**: off by default. Hides Apple's FPS/GPU box
  that iOS can draw over games. If it still shows, close the app fully and reopen it.
- **⋯ → Core settings...** changes a core's options. Some need **Restart the game now**. A save
  state made under different settings is not loaded (it would crash the core); set the option back
  to load it. With Auto-save off, **Restart the game now** starts the game from the beginning.
