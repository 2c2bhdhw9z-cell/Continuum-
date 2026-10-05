# Everything Manic EMU does, and more

The owner's goal: Continuum is the one iPhone emulator everyone uses. Every line here is something
Manic EMU (or another leading iOS emulator) does. Tick it when it is on master. "Phone" means a
phone has shown it working; until then a ticked line is only "in the code".

Ticked `[x]` = on master, not yet shown on a phone unless it says so. Wave 2 (core settings, filters, speeds, discs, every import method, Manic skins and all 48 function buttons, keyboard, motion, remapping, 19 new systems) landed 3 October 2026. Everything ticked is in the current install (build 121, 32 cores). The phone test steps for all of it are in [TESTING.md](../TESTING.md).

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
| WonderSwan / Color | mednafen_wswan | .ws .wsc .pc2 .pcv2 | [x] code |
| Neo Geo Pocket / Color | mednafen_ngp | .ngp .ngc .ngpc .npc | [x] code |
| PC Engine CD, SuperGrafx | mednafen_pce, mednafen_supergrafx | .pce .sgx .cue .ccd .chd .toc .m3u | [x] code |
| Commodore Amiga | puae | .adf .adz .dms .fdi .ipf .hdf .hdz .lha .slave .info .cue .ccd .nrg .mds .iso .chd .uae .m3u .zip .7z .rp9 | [x] code |
| Commodore 64 | vice_x64sc | .d64 .d71 .d80 .d81 .d82 .g64 .g41 .x64 .t64 .tap .prg .p00 .crt .bin .gz .d6z .d7z .d8z .g6z .g4z .x6z .cmd .m3u .vfl .vsf .nib .nbz .d2m .d4m | [x] code |
| DOS | dosbox_pure | .zip .dosz .exe .com .bat .iso .cue .img .ima .vhd .jrc .tc .m3u .conf | [x] code |
| DOOM | prboom | .wad .iwad .pwad | [x] code |
| Atari Jaguar | virtualjaguar | .j64 .jag .rom .abs .cof .bin .prg | [x] code |
| Atari Lynx | handy | .lnx .o | [x] code |
| Atari 7800 | prosystem | .a78 .bin .cdf | [x] code |
| Atari 5200 | a5200 | .a52 .bin | [x] code |
| Arcade | fbneo (mame2003_plus as a second choice) | .zip .7z .cmd | [x] code |
| Pokemon Mini | pokemini | .min | [x] code |
| Virtual Boy | mednafen_vb | .vb .vboy | [x] code |
| Sega Saturn | yabause (mednafen_saturn as a choice) | .iso .chd .ccd .cue | [x] code |
| Sega CD | genesis_plus_gx | .chd .iso .cue | [x] code |
| Sega 32X | picodrive | .32x | [x] code |
| Master System extra | genesis_plus_gx | .bms | [x] code |
| 3DS extra | azahar | .app .cia | [x] code |
| PSP extra | ppsspp | .elf .iso .prx .pbp .chd (ambiguous with PS1, see below) | [x] code |
| DS extra | melonds | .ds | [x] code |
| NES extra | fceumm | .fc | [x] code |
| SNES extra | snes9x | .snes | [x] code |
| Sega Dreamcast | flycast, from source (the buildbot's copy needs JIT) | .cdi .gdi .chd .cue .bin .m3u | [x] code, in the IPA since build 119 |
| Adobe Flash | Ruffle in a bundled player view | .swf | [x] code |
| J2ME | JavaScript J2ME engine in a bundled player view | .jar | [x] code |
| GameCube | Dolphin interpreter | .gcm .gcz .rvz .iso .dol .elf | [ ] researched: not playable without JIT (HARD_SYSTEMS.md) |
| Wii | Dolphin interpreter | .rvz .wbfs .ciso .wia .iso .wad .dol .elf | [ ] researched: not playable without JIT (HARD_SYSTEMS.md) |
| Symbian / N-Gage | EKA2L1 | .sis .sisx .n-gage | [ ] researched: needs a libretro wrapper written, later (HARD_SYSTEMS.md) |

**Shared extensions** (.cue .chd .iso .bin .m3u .zip are used by several systems): the import
looks inside the file where it can (disc header, cue contents, zip contents) and asks the user to
pick a system only when it truly cannot tell. The choice is remembered per game.

## Files

- [x] .zip and .7z import: unpacked on import, except where the core wants the archive itself
      (arcade, DOS, Amiga .zip).
- [x] Save files in Manic's formats, import and export, per system: .srm .sav .dsv .mcd .mcr .eep
      .flash .nvr .bkr .dsg, Dreamcast VMU, PSP and 3DS save folders as zips.
- [x] Multi-disc games: .m3u, and swap disc / insert disc in game.

## Ways to get games in

- [x] Files and iCloud Drive picker (already).
- [x] Wi-Fi transfer: a switch starts a small upload page; type the shown address into any browser.
- [x] Paste from the clipboard (works with Handoff from a Mac).
- [x] Drag and drop into the app.
- [x] Open in / Share to Continuum from other apps.
- [x] WebDAV (NAS, router storage).
- [ ] SMB: taken out after build 117 closed on launch (the SMB library was not packed into the app). Needs redoing.
- [x] Google Drive, Dropbox, OneDrive through the Files picker. [ ] Direct logins need app ids only the owner can register with Google, Dropbox and Microsoft.

## Skins

- [x] `.manicskin` files, Manic's `public.aoshuang.game.*` identifiers, and Delta's.
- [x] One skin used across related systems (GB/GBC, MD/MCD/32X, MS/GG/SG-1000, NES/FDS, DOS/DOOM).
- [x] Default skin per system, a different skin per game, and switching skin mid-game.
- [x] Press animations (`asset.normal` per button).
- [x] Switch buttons: `selected` asset, spring `animation` begin/end, `selfRetracting`, state binding.
- [x] Button sound (`sound.caf`).
- [x] Every custom function button: flex, quickSave, quickLoad, fastForward, toggleFastForward,
      fastForward2x/3x/4x, reverseScreens, volume, saveStates, cheatCodes, skins, filters,
      screenshot, haptics, controllers, orientation, functionLayout, restart, resolution, quit,
      amiibo, homeMenu, toggleControlls, blowing, palette, swapDisk, insertDisc, shake,
      toggleAnalog, retroAchievements, airPlayScaling, airPlayLayout, gameplayManuals, triggerPro,
      tvType, leftDifficulty, rightDifficulty, screenScaling, j2meSettings, dosSettings,
      coreSettings, rewind, slowMotion, wswanRotation, ndsLidToggle, skinButtonBinding.

## In-game features those buttons need

- [x] Core settings screen per core (resolution, renderer, palettes and so on).
- [x] Filters: CRT, scanlines, LCD grid, smooth, and more.
- [x] Palettes for Game Boy, Game Boy Color, Virtual Boy.
- [x] Fast forward speeds 2x, 3x, 4x, cycle, and slow motion.
- [x] Shake (the phone's motion sensor to the core), DS lid, WonderSwan rotation.
- [x] Controller type per port (DualShock and so on).
- [x] Hide or show the controls, orientation lock.
- [x] 3DS home menu.
- [x] Simulated blow for DS mic games.
- [x] Gameplay manuals (a PDF per game).
- [x] Button remapping for skins and controllers (triggerPro-style profiles).
- [x] AirPlay scaling and AirPlay layout choices.

## Already ahead of Manic (keep it that way)

Online play for two phones, cloud sync through any folder, RAM cheat search, 50 save slots with
pictures, rewind, extra buttons placed anywhere, skin editor inside the app, game rumble on the
phone and controllers.

## Still to do

- [x] Flash (.swf) through Ruffle in a bundled player view.
- [x] J2ME (.jar) through a bundled JavaScript J2ME engine.
- [x] GameCube and Wii research: done, docs/HARD_SYSTEMS.md. Not playable without JIT; not built.
- [x] Symbian / N-Gage research: done, docs/HARD_SYSTEMS.md. Possible later, needs a wrapper.
- [x] Dreamcast builds on CI (in the IPA since build 119). [ ] Runs on a phone: TESTING.md C19.
- [ ] SMB file shares, done properly this time (see Ways to get games in).
- [ ] Direct Google Drive / Dropbox / OneDrive logins (needs the owner's developer app ids).
- [x] Achievements on Game Boy Advance (memory maps). Still to fix: .cht RAM cheats on GBA use system RAM addresses.

## Where things stand (5 October 2026, build 121)

- Builds 117 to 121 are done; STATUS.md lists what each one changed. Build 121 is green, version
  0.8.0 (121), 32 cores, and adds a Settings switch that hides Apple's performance overlay.
- STATUS.md, TESTING.md and docs/PLATFORM_LIMITS.md are current. TESTING.md is now numbered phone
  steps (A1 onward) instead of the old queue. The scratch-folder worker reports are no longer needed.
- Next: whatever the owner's phone tests turn up, then the open lines above (SMB, the GBA cheat
  addresses, Dreamcast speed if C19 shows it is slow).
