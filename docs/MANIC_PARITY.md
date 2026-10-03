# Everything Manic EMU does, and more

The owner's goal: Continuum is the one iPhone emulator everyone uses. Every line here is something
Manic EMU (or another leading iOS emulator) does. Tick it when it is on master. "Phone" means a
phone has shown it working; until then a ticked line is only "in the code".

Keep this file current. A new session should be able to read this and carry on.

## Decisions already made

- **New cores come prebuilt from the libretro iOS buildbot**
  (`https://buildbot.libretro.com/nightly/apple/ios-arm64/latest/`), fetched by CI, with each
  file's checksum written into the build metadata. Building twenty more cores from source would
  blow the CI time limit. Cores the buildbot does not carry are built from source like the
  existing twelve.
- **Core settings are answered now.** The engine used to refuse every core setting so each core
  ran on its own defaults. Manic exposes core settings, resolution, palettes and the Atari switches,
  all of which are core settings, so the engine now answers with the core's own default unless the
  user changed it, and Settings shows each core's options.
- **No JIT**, still. Systems that need a recompiler to be playable (GameCube, Wii, Symbian) are
  attempted only as interpreters and are honest about speed.
- **Flash and J2ME run in a bundled player view inside the app** (Ruffle for Flash, a JavaScript
  J2ME engine), the way Manic does. That is a player inside the .ipa, not a web build of
  Continuum, so it does not break the no-web rule.

## Systems

Already in the app: NES, Famicom Disk System, SNES, Game Boy, Game Boy Color, Game Boy Advance,
Master System, Game Gear, SG-1000, Mega Drive, PC Engine HuCard, Atari 2600, PlayStation, N64,
DS, 3DS, PSP.

| System | Core | Formats Manic takes | State |
| --- | --- | --- | --- |
| WonderSwan / Color | mednafen_wswan | .ws .wsc .pc2 .pcv2 | [ ] |
| Neo Geo Pocket / Color | mednafen_ngp | .ngp .ngc .ngpc .npc | [ ] |
| PC Engine CD, SuperGrafx | mednafen_pce, mednafen_supergrafx | .pce .sgx .cue .ccd .chd .toc .m3u | [ ] |
| Commodore Amiga | puae | .adf .adz .dms .fdi .ipf .hdf .hdz .lha .slave .info .cue .ccd .nrg .mds .iso .chd .uae .m3u .zip .7z .rp9 | [ ] |
| Commodore 64 | vice_x64sc | .d64 .d71 .d80 .d81 .d82 .g64 .g41 .x64 .t64 .tap .prg .p00 .crt .bin .gz .d6z .d7z .d8z .g6z .g4z .x6z .cmd .m3u .vfl .vsf .nib .nbz .d2m .d4m | [ ] |
| DOS | dosbox_pure | .zip .dosz .exe .com .bat .iso .cue .img .ima .vhd .jrc .tc .m3u .conf | [ ] |
| DOOM | prboom | .wad .iwad .pwad | [ ] |
| Atari Jaguar | virtualjaguar | .j64 .jag .rom .abs .cof .bin .prg | [ ] |
| Atari Lynx | handy | .lnx .o | [ ] |
| Atari 7800 | prosystem | .a78 .bin .cdf | [ ] |
| Atari 5200 | a5200 | .a52 .bin | [ ] |
| Arcade | fbneo (mame2003_plus as a second choice) | .zip .7z .cmd | [ ] |
| Pokemon Mini | pokemini | .min | [ ] |
| Virtual Boy | mednafen_vb | .vb .vboy | [ ] |
| Sega Saturn | yabause (mednafen_saturn as a choice) | .iso .chd .ccd .cue | [ ] |
| Sega CD | genesis_plus_gx | .chd .iso .cue | [ ] |
| Sega 32X | picodrive | .32x | [ ] |
| Master System extra | genesis_plus_gx | .bms | [ ] |
| 3DS extra | azahar | .app .cia | [ ] |
| PSP extra | ppsspp | .elf .iso .prx .pbp .chd (ambiguous with PS1, see below) | [ ] |
| DS extra | melonds | .ds | [ ] |
| NES extra | fceumm | .fc | [ ] |
| SNES extra | snes9x | .snes | [ ] |
| Sega Dreamcast | flycast, from source (not on the iOS buildbot) | .cdi .gdi .chd .cue .bin .m3u | [ ] |
| Adobe Flash | Ruffle in a bundled player view | .swf | [ ] |
| J2ME | JavaScript J2ME engine in a bundled player view | .jar | [ ] |
| GameCube | Dolphin interpreter, from source | .gcm .gcz .rvz .iso .dol .elf | [ ] research |
| Wii | Dolphin interpreter, from source | .rvz .wbfs .ciso .wia .iso .wad .dol .elf | [ ] research |
| Symbian / N-Gage | EKA2L1 | .sis .sisx .n-gage | [ ] research |

**Shared extensions** (.cue .chd .iso .bin .m3u .zip are used by several systems): the import
looks inside the file where it can (disc header, cue contents, zip contents) and asks the user to
pick a system only when it truly cannot tell. The choice is remembered per game.

## Files

- [ ] .zip and .7z import: unpacked on import, except where the core wants the archive itself
      (arcade, DOS, Amiga .zip).
- [ ] Save files in Manic's formats, import and export, per system: .srm .sav .dsv .mcd .mcr .eep
      .flash .nvr .bkr .dsg, Dreamcast VMU, PSP and 3DS save folders as zips.
- [ ] Multi-disc games: .m3u, and swap disc / insert disc in game.

## Ways to get games in

- [x] Files and iCloud Drive picker (already).
- [ ] Wi-Fi transfer: a switch starts a small upload page; type the shown address into any browser.
- [ ] Paste from the clipboard (works with Handoff from a Mac).
- [ ] Drag and drop into the app.
- [ ] Open in / Share to Continuum from other apps.
- [ ] WebDAV and SMB (NAS, router storage).
- [ ] Google Drive, Dropbox, OneDrive: through Files (works now) and as direct logins.

## Skins

- [ ] `.manicskin` files, Manic's `public.aoshuang.game.*` identifiers, and Delta's.
- [ ] One skin used across related systems (GB/GBC, MD/MCD/32X, MS/GG/SG-1000, NES/FDS, DOS/DOOM).
- [ ] Default skin per system, a different skin per game, and switching skin mid-game.
- [ ] Press animations (`asset.normal` per button).
- [ ] Switch buttons: `selected` asset, spring `animation` begin/end, `selfRetracting`, state binding.
- [ ] Button sound (`sound.caf`).
- [ ] Every custom function button: flex, quickSave, quickLoad, fastForward, toggleFastForward,
      fastForward2x/3x/4x, reverseScreens, volume, saveStates, cheatCodes, skins, filters,
      screenshot, haptics, controllers, orientation, functionLayout, restart, resolution, quit,
      amiibo, homeMenu, toggleControlls, blowing, palette, swapDisk, insertDisc, shake,
      toggleAnalog, retroAchievements, airPlayScaling, airPlayLayout, gameplayManuals, triggerPro,
      tvType, leftDifficulty, rightDifficulty, screenScaling, j2meSettings, dosSettings,
      coreSettings, rewind, slowMotion, wswanRotation, ndsLidToggle, skinButtonBinding.

## In-game features those buttons need

- [ ] Core settings screen per core (resolution, renderer, palettes and so on).
- [ ] Filters: CRT, scanlines, LCD grid, smooth, and more.
- [ ] Palettes for Game Boy, Game Boy Color, Virtual Boy.
- [ ] Fast forward speeds 2x, 3x, 4x, cycle, and slow motion.
- [ ] Shake (the phone's motion sensor to the core), DS lid, WonderSwan rotation.
- [ ] Controller type per port (DualShock and so on).
- [ ] Hide or show the controls, orientation lock.
- [ ] 3DS home menu.
- [ ] Simulated blow for DS mic games.
- [ ] Gameplay manuals (a PDF per game).
- [ ] Button remapping for skins and controllers (triggerPro-style profiles).
- [ ] AirPlay scaling and AirPlay layout choices.

## Already ahead of Manic (keep it that way)

Online play for two phones, cloud sync through any folder, RAM cheat search, 50 save slots with
pictures, rewind, extra buttons placed anywhere, skin editor inside the app, game rumble on the
phone and controllers.
