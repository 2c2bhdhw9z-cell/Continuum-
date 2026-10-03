# Continuum

One app for the iPhone. It plays games from older consoles. You install `Continuum.ipa` yourself. It is not on the App Store.

The current install is [Continuum-102](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-102-31b6729/Continuum-102.ipa). Newer links get posted in chat when a build is ready. Ignore any older note that says the file is about 5 MB or that it only has five cores.

This install is **not** signed with a JIT. Heavier systems here run on an interpreter.

## What is actually true

**Done on a phone**

- NES, SNES, Game Boy Color, Game Boy Advance, Game Gear, Genesis
- Nintendo DS (Mario Kart DS and Pokémon SoulSilver)
- Nintendo 64, software only (Smash reached the character select, about 60 fps, build 97). No JIT.
- PlayStation, the original core (Crash, no BIOS file)

**In the app, not finished**

- PlayStation GPU option (Beetle). Crash did boot with a real `scph1001.bin`. That is not proof the GPU handoff is done. The other PlayStation option still runs with no BIOS.
- Nintendo 3DS (Mario Kart). It runs, then randomly stutters and comes back. That hitch is **not** fixed. Continuum-102 tries not to freeze the game while new graphics finish. Nobody has confirmed that on a phone yet. No JIT.
- On-screen control skins. The system list stays put, and tapping a system does change the layout. Sideways skins are in 102 and are **not** done until they feel right. A skin imported before 102 only has the upright layout until you import it again.

**In the app, not tried on a phone**

- Plain Game Boy (`.gb`), Master System, Famicom Disk System, SG-1000, TurboGrafx-16, Atari 2600
- The OpenGL door (build 101). A core that asks for OpenGL is no longer turned away. The picture is copied onto the screen. No phone has shown that frame. The phone could do OpenGL; the app only built this second door later. Crash’s frames are not proof the picture went through Vulkan.

**Not in the app**

- PSP. Next, not in 102. No JIT. Other people saying PSP “runs fine” is not this app. It is not done until a game runs here.
- Switch. Not built. It is a system to emulate, not a device this app runs on.

The full table, including what is only half done, is [STATUS.md](STATUS.md). [TESTING.md](TESTING.md) is the live checklist. Old notes live in `docs/archive/`. `.kiro` is still in the repo and is outdated.

## How to add games

Open the app, tap Import, and pick the game files. For a PlayStation disc, pick the `.cue` and every `.bin` in the same go. You can also drop files in the iPhone Files app under On My iPhone, Continuum.

A 3DS game has to be a decrypted `.3ds`, `.3dsx`, `.cci`, or `.cxi`. A store copy can also need Nintendo system files this app does not include. Without those, the screen can stay black.

The Beetle PlayStation option needs `scph1001.bin` in that same Continuum folder.
