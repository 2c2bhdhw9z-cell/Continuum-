# Continuum

One app for the iPhone. It plays games from older consoles. You install the `.ipa` yourself (ESign
or a similar installer). It is not on the App Store.

**The install:** always the newest file on the
[Releases page](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest), named
`Continuum-<number>.ipa`. Every push to `master` builds one automatically (about 30 minutes).
As of 5 October 2026 that is build 122 or newer. The version shows as `0.8.0 (<number>)` in the app.

This install is **not** signed with a JIT. Heavier systems run on an interpreter.

## What is actually true

**Done on a phone**

- NES, SNES, Game Boy Color, Game Boy Advance, Game Gear, Genesis
- Nintendo DS (Mario Kart DS and Pokémon SoulSilver)
- Nintendo 64, software only (Smash reached the character select, about 60 fps). No JIT.
- PlayStation, both cores (Crash, with and without a BIOS)
- Nintendo 3DS: Mario Kart runs (it can stutter after a transition)
- Rewind, fast forward (about 4x), sound, controllers, auto-save and resume, typed cheats
- The **+** button opens Files, the in-game **⋯** menu, the layout editor, the system pickers
- TV/AirPlay picture (sharper since build 119)
- Landscape with no skin imported (fixed in build 120)
- A 3DS skin sideways with both screens in their own holes (owner's screenshot, 5 October)

**In the app, not confirmed on a phone yet**

Everything in [TESTING.md](TESTING.md)'s list, in short: 20+ more systems (32 cores in total,
including PSP, Dreamcast, Saturn, arcade, computers, Flash and J2ME), Wi-Fi transfer and other ways
to import, skin holes and Manic skins, extra buttons, the 50-slot save manager, battery saves,
cheat search, two-screen layouts, online play, RetroAchievements, cloud sync, the Azahar
settings-change crash fix (build 120), the switch that hides Apple's performance overlay
(build 121), and build 122's batch: the button that hides the player's top bar, importing several
skins at once, and the fixes listed in STATUS.md.

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

A 3DS game has to be a decrypted `.3ds`, `.3dsx`, `.cci`, or `.cxi`. A PSP game in this build is a
`.cso`; an `.iso` is treated as PlayStation. BIOS files (for example `scph1001.bin` for the Beetle
PlayStation option) go in that same Continuum folder; the app names the exact file when one is
missing.

## Settings worth knowing

- **Settings → DIAGNOSTICS → Apple performance overlay**: off by default. Hides Apple's FPS/GPU box
  that iOS can draw over games. If it still shows, close the app fully and reopen it.
- **⋯ → Core settings...** changes a core's options. Some need **Restart the game now**. A save
  state made under different settings is not loaded (it would crash the core); set the option back
  to load it.
