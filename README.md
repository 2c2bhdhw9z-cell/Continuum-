# Continuum

One app for the iPhone. It plays games from older consoles. You install `Continuum.ipa` yourself. It is not on the App Store.

The newest file on the releases page is [Continuum-107](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-107-53240a8/Continuum-107.ipa). That one is the older PSP compile. It does not contain the skin fix. Do not install it for skins. The skin install is not up yet. The link gets posted in chat when it is.

This install is **not** signed with a JIT. Heavier systems here run on an interpreter.

## What is actually true

**Done on a phone**

- NES, SNES, Game Boy Color, Game Boy Advance, Game Gear, Genesis
- Nintendo DS (Mario Kart DS and Pokémon SoulSilver)
- Nintendo 64, software only (Smash reached the character select, about 60 fps, build 97). No JIT.
- PlayStation, the original core (Crash, no BIOS file)

**In the app, not finished**

- PlayStation GPU option (Beetle). Crash did boot with a real `scph1001.bin`. That is not proof the GPU handoff is done. The other PlayStation option still runs with no BIOS.
- Nintendo 3DS (Mario Kart) runs. The open problem is the skin, below.
- On-screen control skins. On the last phone report, Game Boy Color sideways works and other systems do not: the 3DS picture sits above the skin and the bottom screen stays empty. A fix is in the tree (`ee307b8`) and is not installed yet. Not done until the picture is in the hole on the phone.

**In the app, not tried on a phone**

- Plain Game Boy (`.gb`), Master System, Famicom Disk System, SG-1000, TurboGrafx-16, Atari 2600
- PSP (PPSSPP). IR interpreter, no JIT, no dynarec, no executable memory, no BIOS. `.cso` only. `.iso`, `.chd` and `.pbp` stay PlayStation. Not device-proven. Someone else's PPSSPP running smoothly is not this phone. It is not done until a game runs here.
- The OpenGL door (build 101). A core that asks for OpenGL is no longer turned away. The picture is copied onto the screen. No phone has shown that frame. The phone could do OpenGL; the app only built this second door later. Crash’s frames are not proof the picture went through Vulkan.

**Not in the app**

- Switch. Not built. It is a system to emulate, not a device this app runs on.
- JIT. Left out on purpose.

**The rest of the bar**

Done on a phone: rewind, and fast forward at about 4x (not 5x).

In the tree, not on a phone (`ee307b8`): each screen in the hole the skin names, a circle pad as a real stick, and a pressed button picture when the file has one. Import the skin again after installing, or the old save keeps a single hole and no stick.

Not built: swapping which screen is big, AirPlay, extra buttons you place yourself, editing an imported skin in the app, touch as a mouse, camera, microphone, Amiibo, haptics, a 50-slot save manager, exporting a save, cheat search, importing a cheat file, online play, achievements, and cloud sync. Typing a cheat code does exist.

The line-by-line table is in [STATUS.md](STATUS.md). [TESTING.md](TESTING.md) is the live checklist. Old notes live in `docs/archive/`. `.kiro` is still in the repo and is outdated.

## How to add games

Open the app, tap Import, and pick the game files. For a PlayStation disc, pick the `.cue` and every `.bin` in the same go. You can also drop files in the iPhone Files app under On My iPhone, Continuum.

A 3DS game has to be a decrypted `.3ds`, `.3dsx`, `.cci`, or `.cxi`. A store copy can also need Nintendo system files this app does not include. Without those, the screen can stay black.

A PSP game in this build is a `.cso`. An `.iso` is still PlayStation. Renaming an `.iso` to `.cso` does not make it one.

The Beetle PlayStation option needs `scph1001.bin` in that same Continuum folder.
