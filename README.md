# Continuum

**An all-in-one emulator for iPhone, in public beta.** 38 systems in one app, from the NES to the
Nintendo 3DS, with skins, save states, rewind, fast forward, cheats, controllers, AirPlay to a TV
and online play with a second phone.

It is not on the App Store: you install the `.ipa` yourself.

## Download

**[Get the newest build](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest)**, the file
named `Continuum-<number>.ipa`. Sign and install it with your usual sideloading app and your own
certificate. Your version number is in Settings, About.

Games and BIOS files are **not** included. Use your own.

## Systems

| Plays well on a phone already | In the app, needs testers |
| --- | --- |
| NES, SNES, Game Boy, Game Boy Color, Game Boy Advance | PSP, Dreamcast, Saturn, Sega CD, 32X |
| Mega Drive / Genesis, Game Gear | Master System, SG-1000, Famicom Disk System |
| PlayStation | Arcade (FinalBurn Neo, MAME 2003-Plus) |
| Nintendo 64 | WonderSwan, Neo Geo Pocket, Lynx, Atari 7800 and 5200, Virtual Boy |
| Nintendo DS | PC Engine CD, SuperGrafx |
| Nintendo 3DS (decrypted games) | Commodore 64, Amiga, DOS, DOOM |
| TurboGrafx-16, Atari 2600, Jaguar, Pokemon Mini | Flash (.swf) and old phone games (.jar) |

Not supported: GameCube, Wii and Switch. Heavier systems can be slow without JIT. If you can turn
JIT on (StikDebug or similar), Continuum uses it by itself; see Settings, Technical details.

## Adding games

Tap **+** in the Library and pick your files (zip and 7z work too). For a disc game made of a
`.cue` and `.bin` files, pick the `.cue` and every `.bin` together. You can also put files in the
Files app under On My iPhone, Continuum. Hold your finger on **+** for Wi-Fi transfer from a
computer, pasting, and WebDAV servers.

A system that needs a BIOS file says which one when you open a game. Put it in the same Continuum
folder, then Settings, BIOS, Install a BIOS from the Continuum folder.

## Helping test

What helps most: try a system from the right-hand column, or a game nobody has tried, and say how
it went.

- **In a game:** ⋯ then **Send feedback about this game**. Rate how it runs, tick what is wrong,
  and draw on the picture to point at the problem. The app's details are added for you.
- **Anything else:** Settings, **Send feedback** (the first card).
- **If the app closes by itself,** it offers to send a crash report the next time it opens.

Reports go by email to idkplswrk@gmail.com; you see the whole message before it is sent.

## For development

[STATUS.md](STATUS.md) is what is finished, system by system. [TESTING.md](TESTING.md) is the
owner's test list. [docs/PRODUCT_SCOPE.md](docs/PRODUCT_SCOPE.md) is what the app is meant to be,
and [docs/MANIC_PARITY.md](docs/MANIC_PARITY.md) is the feature checklist. Builds come from GitHub
Actions; every build of `master` that changes the app becomes a Release.

## License

Continuum is licensed under the GNU General Public License v3.0; see [LICENSE](LICENSE). The
emulator cores and libraries inside the app keep their own licenses (listed in the app under
Settings → About → Open-source credits). Some cores (Snes9x, Genesis Plus GX, PicoDrive,
FinalBurn Neo, MAME 2003-Plus) allow free distribution only, never sale.
