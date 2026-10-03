# Continuum

One app for the iPhone. It plays games from older consoles. You install `Continuum.ipa` yourself. It is not on the App Store.

The install is [Continuum-111](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-111-cdae676/Continuum-111.ipa). It has everything 109 had (the skin fixes) plus the whole batch below. Build 110 failed and has no file. Import the skin file again after installing. The skin is not done until the picture is in the hole on the phone.

This install is **not** signed with a JIT. Heavier systems here run on an interpreter.

## What is actually true

**Done on a phone**

- NES, SNES, Game Boy Color, Game Boy Advance, Game Gear, Genesis
- Nintendo DS (Mario Kart DS and Pokémon SoulSilver)
- Nintendo 64, software only (Smash reached the character select, about 60 fps, build 97). No JIT.
- PlayStation, the original core, ran Crash with no BIOS file.
- PlayStation Beetle ran Crash on the phone with `scph1001.bin` (build 98). That BIOS boot happened.

**In the app, not finished**

- PlayStation GPU handoff (Beetle). The BIOS boot above is real. What is not finished is the GPU handoff, not the boot. The other PlayStation option still runs with no BIOS.
- Nintendo 3DS (Mario Kart) runs. The open problem is the skin, below.
- On-screen control skins. On the last phone report, Game Boy Color sideways works and other systems do not: the 3DS picture sits above the skin and the bottom screen stays empty. The file to install is Continuum-109. It is not proven on a phone. Not done until the picture is in the hole on the phone.

**In the app, not tried on a phone**

- Plain Game Boy (`.gb`), Master System, Famicom Disk System, SG-1000, TurboGrafx-16, Atari 2600
- PSP (PPSSPP). IR interpreter, no JIT, no dynarec, no executable memory, no BIOS. `.cso` only. `.iso`, `.chd` and `.pbp` stay PlayStation. Not device-proven. Someone else's PPSSPP running smoothly is not this phone. It is not done until a game runs here.
- The OpenGL door (build 101). A core that asks for OpenGL is no longer turned away. The picture is copied onto the screen. No phone has shown that frame. The phone could do OpenGL; the app only built this second door later. Crash’s frames are not proof the picture went through Vulkan.

**Not in the app**

- Switch. Not built. It is a system to emulate, not a device this app runs on.
- JIT. Left out on purpose.

**The rest of the bar**

Done on a phone: rewind, and fast forward at about 4x (not 5x).

In 109, not proven on a phone: each screen in the hole the skin names, a circle pad as a real stick, and a pressed button picture when the file has one. Import the skin again after installing, or the old save keeps a single hole and no stick.

Built after 109, in the next install, and not tried on a phone: swapping which screen is big (six DS and 3DS layouts), AirPlay to a TV, extra buttons you place yourself (combos, turbo, quick save and more), editing an imported skin in the app, touch as a mouse, the microphone for 3DS games, haptics and game rumble, a 50-slot save manager, exporting and importing saves, cheat search, importing a cheat file, online play for two phones, RetroAchievements, and cloud sync through any folder in Files. In-game battery saves are now written to disk too; before this they only lived inside save states.

Built but held back by the cores, not the app: the camera (the 3DS core never asks for it) and Amiibo (the 3DS core has no way to take one). Achievements are partial: Game Boy Advance sets will not trigger correctly yet.

The line-by-line table is in [STATUS.md](STATUS.md). [TESTING.md](TESTING.md) is the live checklist. What the owner wants built, and what the project will and will not become, is in [docs/PRODUCT_SCOPE.md](docs/PRODUCT_SCOPE.md). Old notes live in `docs/archive/`. `.kiro` is still in the repo and is outdated.

## How to add games

Open the app, tap Import, and pick the game files. For a PlayStation disc, pick the `.cue` and every `.bin` in the same go. You can also drop files in the iPhone Files app under On My iPhone, Continuum.

A 3DS game has to be a decrypted `.3ds`, `.3dsx`, `.cci`, or `.cxi`. A store copy can also need Nintendo system files this app does not include. Without those, the screen can stay black.

A PSP game in this build is a `.cso`. An `.iso` is still PlayStation. Renaming an `.iso` to `.cso` does not make it one.

The Beetle PlayStation option needs `scph1001.bin` in that same Continuum folder.
