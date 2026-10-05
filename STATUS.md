# What is finished, and what is not

One page, kept current, so nothing has to be inferred from a commit log. Last updated
5 October 2026 (build 123). The install is always the newest file on the
[Releases page](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest).

Three states only:

- **Done** means built, in the `.ipa`, and confirmed working on a device.
- **Built, untested** means it is in the app and should work, and nobody has tried it yet. Those
  are the rows in [TESTING.md](TESTING.md)'s queue.
- **Partial** means some of it works and a named piece is missing. Read the note.

If a row says Partial, the note says exactly what is absent. Nothing here is rounded up.

For what the owner wants built, and the scope rules this page works inside, see
[docs/PRODUCT_SCOPE.md](docs/PRODUCT_SCOPE.md).

---

## Systems

**32 cores** are in the IPA in builds 119 to 122 (checked in the build 122 file itself: version 0.8.0 (122), 32 core files, every emulator at its pinned version). The table below is the
first seventeen systems; the 3 October batch below adds the rest. Original note:
**Twelve** cores, **seventeen** systems, at the time this table was written. The sixteenth is the **Nintendo 3DS**. The seventeenth is the **PSP**, on PPSSPP. Two of the older systems cost no new emulator at all, both having been supported already by cores in the app and simply not wired up. One system is still ahead and is not in this
table: **the Switch**, hardest last. The N64 arrived by the software-renderer trick. The 3DS did not: it is Azahar's Vulkan `set_image` path, the same hook Beetle uses, with the CPU JIT compiled out. It is in the IPA. **Device (Brett, builds 100 and 101):** Mario Kart runs, then stutters after a transition and comes back. It does not stay locked. **Not Done.** PSP is in the IPA as Partial. It has not been tried on a phone.

| System | Core | State | What is missing |
| --- | --- | --- | --- |
| NES | fceumm | **Done** | |
| SNES | snes9x | **Done** | |
| Game Boy | mgba | **Built, untested** | No `.gb` file has ever been imported. GBC and GBA on the same core are confirmed, so this is a formality rather than a doubt |
| Game Boy Color | mgba | **Done** | |
| Game Boy Advance | mgba | **Done** | |
| Master System | genesis_plus_gx | **Built, untested** | No `.sms` file has ever been imported. Game Gear and Mega Drive on the same core are confirmed |
| Game Gear | genesis_plus_gx | **Done** | |
| Mega Drive / Genesis | genesis_plus_gx | **Done** | |
| PlayStation | pcsx_rearmed | **Done** | Interpreter, not the recompiler. Fast enough; see Recompiler below. **Beetle PSX HW** is a separate core. Build 98 device: Crash **did boot** on that core with `scph1001.bin`, ~60 fps, frames ~2900. An old save from the other PlayStation option would not resume, so it started fresh. The game page **labeled ReARMed** while Beetle was running. Continuum-100 names the core Settings will actually launch, on the game page and in the Library detail. Brett has not confirmed that label on the phone. Same screen: hardware-render `installed=false`. That line means the GPU handles were not handed over yet. Crash still drew ~2900 frames while it was false, so it is **not** “no picture.” Those frames are also **not** proof the picture went through Vulkan. The “first Vulkan frame” line has not been seen on a phone. Do not stamp the GPU hook Done off either reading. Soft PS1 still runs without a BIOS. See road step 4 |
| **Famicom Disk System** | fceumm | **Built, untested** | Needs `disksys.rom`, which is Nintendo's own code and cannot ship with the app. The launch path checks for it by name and says so rather than letting the core fail |
| **Sega SG-1000** | genesis_plus_gx | **Built, untested** | Needs nothing extra |
| **Nintendo 64** | parallel_n64 | **Done** | Device-proven on build 97 (`258a828`): past `N64 first tick…`, frames climbing, ~60 fps into Smash character select. Soft/interp only (no JIT on this signed IPA). `.n64`, `.z64`, `.v64` |
| **TurboGrafx-16** | mednafen_pce_fast | **Built, untested** | HuCard games only. PC Engine CD needs a system card BIOS that cannot ship |
| **Atari 2600** | stella2023 | **Built, untested** | `.a26` only. A 2600 ROM named `.bin` has to be renamed, because `.bin` belongs to the PlayStation here as a disc track |
| **Nintendo 3DS** | azahar | **Partial** | A game runs (Mario Kart). Skin holes for 3DS are built and not confirmed on a phone. Build 119 crashed when a game was reopened after changing a "restart required" Azahar setting; the likely cause was the auto-save from the old settings being loaded. Build 120 refuses such a state with a message instead. Not yet confirmed on a phone. No JIT. Decrypted `.3ds`, `.3dsx`, `.cci`, `.cxi` only. A retail game can still need 3DS system archives this app does not ship |
| **Nintendo DS** | melonDS | **Done** | Confirmed on device (build 80): Mario Kart DS and Pokémon SoulSilver, dual screens live, ~60 fps, 0 dropped |
| **PlayStation Portable** | ppsspp | **Partial** | In the IPA as `ppsspp_libretro_ios.dylib`. CPU is the IR interpreter: the core's option value "IR JIT" is `CPUCore::IR_INTERPRETER` with compile-to-native off. No dynarec, no executable memory. Picture is Vulkan `set_image`, the same hook as the 3DS. No BIOS is shipped; PPSSPP does not need one. `.cso` only. `.iso`, `.chd` and `.pbp` stay PlayStation. **Not device-proven.** Do not claim a game runs or quote a frame rate |

### Nintendo DS, in detail

Broken out rather than left as one row. Device-proven on build 80:

| Piece | State |
| --- | --- |
| Core compiles and links for iOS arm64 | **Done**, 5.7 MB, and it exports `retro_run` |
| Shipped in the `.ipa` | **Done** |
| `.nds` recognised, imported and routed | **Done**. Mario Kart DS and SoulSilver imported and launched on device |
| Both screens drawn | **Done**. Dual screens live in the player shots |
| Buttons, D-pad, L and R, Select and Start | **Done**. Playable on device |
| **Touch screen, engine side** | **Done**. `RETRO_DEVICE_POINTER` did not exist at all; it is now in the input layer, merged per source, exported as `applyPointer`, and covered by 8 unit tests |
| **Touch screen, app side** | **Done**. Stylus path used in play on device |
| **Touch screen, switched on in the core** | **Done**. It was off: the core's touch mode starts at *disabled*, not at the mouse control it advertises, so the screen was dead inside the core regardless of what the app sent. Now answered explicitly, with tests |
| Boots the cartridge rather than the firmware menu | **Done**. The same trap: *boot game directly* advertises enabled and starts off, which would have sent the core to a firmware menu that a generated firmware cannot launch a game from |
| BIOS | **Answered, no files needed**. This build carries a FreeBIOS and generates a firmware when the dumps are absent. The three names stay declared so Settings still reports what it finds, and none is a fine answer |
| Speed | **Done for the titles tried**. Mario Kart DS and SoulSilver held ~60 fps with 0 dropped on build 80 (software rendered, single threaded). Threaded renderer remains a lever if a heavier title is slow |

---

## Features

| Feature | State | What is missing |
| --- | --- | --- |
| Import, including multi-file `.cue` plus `.bin` | **Done** | |
| Library, cover art, the detail card | **Done** | |
| Cover art from the internet | **Done** | |
| Cover art from your own file | **Done** | |
| Cover art captured from the running game | **Done** | Confirmed on the phone with build 122. Build 123 makes it a sharp picture of the game alone (no skin layout around it); that version is TESTING.md A1 step 3 |
| On-screen controls | **Done** | |
| Physical controllers | **Done** | Including a controller and thumbs at the same time |
| Sound | **Done** | |
| Volume and mute | **Done** | |
| Screen fit and scaling | **Done** | |
| Fast forward | **Done** | Tops out near 4x. Past that the engine drops frames instead of going faster, and the menu stops at 4x. That is not 5x |
| Rewind | **Done** | |
| Save states, slots, delete | **In the tree, not on a phone** | 50 fixed slots per game plus the auto-save, each with a picture, date and core. Save, load, overwrite (asks first), rename, delete. Old numbered saves move into free slots and nothing is deleted. Export and import a state file, and the game's own battery save (`.srm`). Saving to the next free slot confirmed on the phone (build 122). Every slot picture was black in 122 (the picture was drawn in a format the drawing step was not built for, so it was thrown away); fixed in build 123, TESTING.md A1. Rename, overwrite, load, delete, export and import not on a phone yet |
| Auto-save and resume | **Done** | |
| Cheats | **In the tree, not on a phone** | Typing a code works on a phone. New and not on a phone: import a RetroArch `.cht` file, and a RAM search (lives, money and so on) that turns an address into a cheat. Up to 128 codes per game. Build 122: a `.cht` file's RAM cheats now land where RetroArch puts them (on GBA they used to hit the wrong memory) |
| On-screen control layout editor | **Partial** | **Done bar (Brett):** every control in the skin file works, not only the ones he names. Picture in the screen hole both ways you hold the phone. Two screens when the skin has two. A joystick or circle pad is a real stick, not a dead picture. Shoulders too. Debug text off the picture. **Device, 5 October (a 3DS skin, Mario Kart 7, sideways):** both screens are in their own holes. The only complaint left was the player's top bar (name and buttons) sitting over the picture. **Build 122, not on a phone yet:** a button next to Back hides or shows the whole top bar; built-in buttons the skin does not name are taken off the screen instead of sitting faintly on the picture; the false "touch layout overlap" line is gone when a skin is in use. Portrait and the other systems' skins are still not confirmed. Do **not** stamp Done until the owner says the skin is right |
| Battery saves (the game's own save) | **In the tree, not on a phone** | Found broken while building the save manager: in-game saves (Pokemon, Zelda, PS1 memory card) were never written to disk, so they only survived inside a save state. Now restored before the first frame and written when you leave or switch apps |
| Save state compatibility refusal | **Done** for the 3DS settings case | Refuses a state from a different core or core build, and (since build 120) one saved under different restart-required core settings. Confirmed on the phone 5 October: changing a 3DS restart setting no longer closes the app. Exported and re-imported states carry their settings since build 122 (TESTING.md A2, not on a phone yet) |
| Hide the player's top bar | **Done** | Build 122, confirmed on the phone 5 October: the eye button next to Back hides and shows the top bar, and the choice is remembered between games |
| Import several skins at once | **Done** | Build 122, confirmed on the phone 5 October (skin library, Import skins) |
| A paused game stays paused after leaving the app | **Done** | Build 122, confirmed on the phone 5 October |
| Wi-Fi transfer access code | **Built, untested** | Build 122. The address now ends in a short code that changes every time Wi-Fi transfer is switched on; anything without it gets nothing, so nobody else on the Wi-Fi can upload files or download saves. TESTING.md B1 |
| Apple performance overlay switch | **Built, untested** | Build 121. Settings → DIAGNOSTICS. Hides Apple's Metal Performance HUD on the game layers and turns off the launch-time request for it. May need the app reopened |
| Landscape with no skin | **Done** | Froze in build 119 (an endless layout loop from a repeated warning line). Fixed in build 120 and confirmed on the phone |
| **+** opens Files, the ⋯ menu, TV picture quality | **Done** | Confirmed on build 119 |
| Honouring what a core wants its content as | **Done** | Every core used to be handed a file path and no bytes. That worked for the first six by luck, and would have given Stella a zero-byte ROM, because it copies straight from the data pointer with no path fallback. The engine now reads what each core declares and loads the file when the core wants bytes, so the next such core needs no change |
| File formats per system | **Done** | Every extension is now taken from the cores' own declared lists rather than a hand-written one. That added the two systems above plus `.smd`, `.swc`, `.fig`, `.unf`, `.unif`, `.sgb`, `.mdf` and `.toc`, which were being refused despite being supported |
| Core setting overrides | **Done** | The host refuses a core's requests for its settings, so every core keeps its own defaults. Two DS settings had to be answered because they do not start at the default they advertise; everything else, for every core, is still refused |
| Multi-screen compositor | **In use, not on a phone** | Now drives the DS and 3DS layouts and the screen swap. Skin holes are a different path (`ee307b8`) and are not proven on a phone |
| Android `.apk` | **Not started** | The one other PLATFORM, and the only one after the iPhone. Next in order, still far off in time. Everything new goes in the Rust engine so Android inherits it |
| Switch wrapper (to EMULATE the Switch) | **Partial** | `native/switch-wrapper/` has the frame gate, a Vulkan stub and a test harness, with no engine behind it. Steps 10 to 12 of the road below |


## The other app's list

This is the bar. A line is **Done** only when a phone showed it. **In the tree** means the code is on master and no phone has run it. **Not built** means it is not in the app. JIT is **out on purpose**, not a missing feature.

Every row below is in the current install (newest on the Releases page). A skin saved on the phone before build 109 only has the first hole and no circle pad, so import the skin file again.

| What | State | Notes |
| --- | --- | --- |
| Two screens, each placed where the skin file says | **Shown on a phone, sideways, 3DS** | The owner's 5 October screenshot (Mario Kart 7, 3DS skin, sideways) has both screens in their holes. Upright, and DS skins, not shown yet |
| Swap which screen is the big one | **In the tree, not on a phone** | DS and 3DS. Six layouts in Settings (stacked, side by side, big top, big bottom, top only, bottom only) and a swap button in the player. Touch follows the bottom screen. With a two-hole skin the pictures trade holes |
| AirPlay: game on the TV, touch screen on the phone | **Partial** | Build 119: the TV picture is confirmed sharper on the phone. TV scaling/layout options not confirmed yet. | AirPlay or a cable. Game on the TV, controls on the phone. On DS and 3DS the phone keeps the touch screen. Two switches in Settings |
| Button shows a pressed picture | **In 109, not on a phone** | Only when the skin file has that picture. Not on a phone |
| Extra buttons you place yourself | **In the tree, not on a phone** | In the layout editor: add a button, a combo, a turbo button, or an action (quick save, quick load, fast forward, rewind, screenshot, pause). Drag, resize, fade, delete. Per system and per way you hold the phone |
| Edit an imported skin inside the app | **In the tree, not on a phone** | Move and resize every button, stick and screen hole, change what a button presses, fade the skin, reset to the file. The imported file is never changed |
| Circle pad or joystick as a real stick | **In 109, not on a phone** | Not a D-pad. Not on a phone. Import the skin again |
| Touch screen as a mouse | **In the tree, not on a phone** | Per system in Settings. Drag moves, tap clicks, two fingers right click. Only games that support a mouse respond (Mario Paint, PlayStation mouse games) |
| iPhone camera into a 3DS game | **App side built, no core uses it** | The app can feed the camera to a core, front or back. The 3DS core (Azahar) never asks for a camera, so no game sees it yet |
| iPhone microphone | **In the tree, not on a phone** | 3DS games that listen (Azahar asks for it). Switch in Settings, off by default. DS games do not use it: melonDS only fakes a blow on its L2 button |
| Amiibo file | **Partial** | Import and pick Amiibo files in the 3DS menu. The 3DS core (Azahar) has no way to receive one yet, and the app says so when you tap |
| Haptics on a button press | **In the tree, not on a phone** | Off, light, medium or strong in Settings. Also game rumble on the phone and on controllers, with its own switch |
| JIT | **Out on purpose** | Not in this signed app. Do not add it to close this list |
| Rewind | **Done** | |
| Fast forward | **Done** | About 4x, not 5x |
| Save slots, including export | **In the tree, not on a phone** | 50 slots plus the auto-save, export and import of states and battery saves |
| Cheats: search, and importing a file | **In the tree, not on a phone** | RAM search and `.cht` import are built. Typing a code already worked |
| Online play | **In the tree, not on a phone** | Two phones, same game. Host or join on the same Wi-Fi (nearby list) or by address. Over the internet the host must open TCP port 55435. No rollback, so lag shows as short stalls. Rewind, fast forward and loading states are off while online. Build 122: if player 2 drops out, the host keeps playing and lets them rejoin on the same address |
| Achievements | **Partial** | RetroAchievements login, unlock banners and a list on the game card. Never tried against the real server. Game Boy Advance achievements will not trigger correctly yet |
| Cloud sync | **In the tree, not on a phone** | Pick any folder in Files (iCloud Drive, Google Drive, Dropbox) once. Save states, battery saves, cheats, settings and covers sync both ways; since build 122 also Flash and J2ME saves, PDF manuals, Amiibo files, the remembered "which system is this" answers and saved servers (their passwords stay on each phone). Kept per phone on purpose: skins, the RetroAchievements login, favourites, the last online-play address, mic and camera permission. Conflicts keep both copies. Nothing is ever only deleted |


---

## Build 124 (5 October 2026)

A bug sweep, nothing new to learn. TESTING.md A4 to A7.

- Restarting from Core settings ("Restart the game now"): the game's picture no longer loses its
  place in the skin. With auto-save off, the game now starts from the beginning and says so; before,
  it could jump back to an old auto-save.
- Error messages in the small text read as plain sentences. They used to show the engine's raw
  form, `SaveState(reason: "...")`, inside the sentence.
- Online play: every change that only this phone would make (core settings, palette, resolution,
  disc swaps, the controller type, analog mode, shaking, mouse mode and so on) is now refused with
  the same sentence, "… is off during online play: the other phone would not do the same". Before,
  some of them went through and the two games drifted apart.
- Crash safety in the engine: three requests a core can make without a place to write the answer
  no longer crash; a damaged or hostile CSO or CHD disc image can no longer make the app ask for
  more memory than the file could fill; a J2ME save is checked against the bytes actually read.
- J2ME and Flash: a J2ME save that took close to 3 seconds to hand back as the game closed was cut
  off and lost; the app now waits longer than the game does. A late save can no longer land on top
  of the next session's. Exporting a running game's save gives the game as it is now, not the last
  automatic save, and importing a save while that game is still closing waits for it.
- RetroAchievements: the background timer runs only while logged in; a login the server turns down
  is cleared and says to log in again, while simply being offline keeps it; the login button cannot
  be pressed twice; drawing a game's achievement card no longer holds up the running game.
- BIOS: a BIOS named in capitals is found by the launch check and the BIOS line, not only by the
  Settings checklist; with Beetle selected, the BIOS line describes Beetle.
- Import: "Which system?" no longer offers GameCube, Wii or Symbian, which cannot run here.
- Exports: a file still open in the share sheet (AirDrop, Save to Files) was deleted the moment
  anything else was exported. Each export now gets its own folder, cleared after 10 minutes.
- Cloud sync: an upload used to delete the cloud copy first, so a failed copy left none, which the
  next sync could take as a deletion. It now copies beside it and swaps it in at the end.
- Controllers: the controllers screen opened from a game and the one opened from the Library can no
  longer both appear; a hardware keyboard's Caps Lock is read correctly; keys are released when the
  app goes to the background.
- Covers: with "Look up cover art" off, nothing is downloaded anywhere, including the cover chooser;
  closing the chooser straight after tapping a cover no longer cancels it; a list that could not be
  downloaded is no longer reported as "no cover found", and is not retried over and over.
- The SMB message now gives the real reason it is missing.

## Build 123 (5 October 2026)

- Save slot pictures were black: every capture (slot pictures, and on some screens covers and
  screenshots) was drawn with the screen's drawing step into a picture of a different format, which
  the phone refuses, so nothing was drawn. Captures now have their own drawing step, and take the
  game's picture alone in its own shape rather than the skin's layout. Slot pictures are sharper
  (288 pixels tall, was 144).
- Build 122 confirmed on the phone: the top-bar hide button, pause staying paused, the 3DS skin
  sideways with nothing on the picture, several skins at once, the tidier small text, the 3DS
  settings change, fast forward, the CRT filter and the cover from the game.

## Build 122 (5 October 2026)

- The owner's asks: an eye button that hides or shows the player's top bar, and importing several
  skins at once.
- Skins: built-in buttons a skin does not name no longer sit faintly over the picture, and the
  false "touch layout overlap" line is gone while a skin is in use.
- Saves: an exported and re-imported state keeps the core settings it was saved under, so the
  3DS settings-change protection covers it too. Cloud sync now carries Flash and J2ME saves,
  manuals, Amiibo and three settings it used to miss, and no longer carries the RetroAchievements
  login name to another phone.
- Safety and battery: Wi-Fi transfer needs the code in its address; the tilt sensor stops when a
  game ends; the microphone and camera cannot be left on by a start that finished after the game
  ended; Amiibo imports no longer overwrite a tag with the same name.
- Smaller: a paused game stays paused after leaving the app; online play lets player 2 rejoin;
  a cover that fails to download can no longer wipe a hand-picked one, and an automatic lookup
  can no longer undo a cover the user just chose; deleting a game removes its cover and the
  manual the app saved for it; two manuals with the same name no longer overwrite each other;
  RetroArch `.cht` RAM cheats use RetroArch's own addressing (the GBA bug).
- On screen: the diagnostic block's first line says "Continuum" once; startup only warns about
  missing cores when one the build must carry is missing.
- Build: every emulator built from source is frozen at the exact version build 121 used, and the
  19 downloaded ones are checked against build 121's checksums (with a frozen copy kept on the
  `core-mirror-1` pre-release), so an outside change cannot break or change a build. A second push
  now waits for the running build instead of cancelling it. The engine's 571 tests, the Switch
  wrapper harness and the skin and player checks now run on every build, first, on a cheap Linux
  machine; a failure stops the build before the 30-minute Mac part.
- Other AI tools' files and branches were removed from the repository.

## Builds 117 to 121 (4 October 2026)

- **117** closed on launch: the SMB library was linked but not packed into the app. SMB removed; CI
  now fails any build that links a framework it does not carry.
- **118** failed to build (Dreamcast core, wrong OpenGL header). Fixed.
- **119**: Dreamcast (flycast) in the IPA, **+** opens Files directly (Import sheet on long press),
  the ⋯ menu no longer rebuilds every frame, sharper TV/AirPlay picture. Confirmed on the phone.
- **120**: no-skin landscape freeze fixed (confirmed); save states saved under different
  restart-required core settings are refused instead of crashing Azahar (not yet confirmed).
- **121**: switch to hide Apple's performance overlay; docs brought up to date.

## Built 3 October 2026, not on a phone yet

Everything below is on master and in the current install. TESTING.md has an easy numbered list for
trying each one.

- **19 new systems:** WonderSwan, Neo Geo Pocket, PC Engine CD, SuperGrafx, Amiga, C64, DOS, DOOM, Jaguar, Lynx, Atari 7800, Atari 5200, Arcade, Pokemon Mini, Virtual Boy, Saturn, Sega CD, 32X, and Dreamcast (optional, never compiled before build 116).
- **Getting games in:** Wi-Fi transfer, paste, drag and drop, Open in, WebDAV, zip and 7z files, automatic system detection, and save files in other emulators' formats. (SMB was in this list and is out; see Not done.)
- **Manic skins:** .manicskin files, a skin library, a skin per game, switching mid-game, press animations, switch buttons, button sounds, and all 48 function buttons.
- **Core settings for every core:** filters, palettes, 2x/3x/4x and slow motion, disc swap, rotation, and separate TV settings.
- **Controls:** a keyboard, tilt and shake, controller types, remapping profiles, DS lid and blow, and the 3DS HOME button.
- **Gameplay manuals:** PDF manuals attached to a game.
- **Also in the current install:** Flash, J2ME, memory maps (GBA achievements and RAM search on GBA), and better disc hashing for achievements.

### Not done

- SMB (NAS shares) is out again: build 117 closed on launch because the SMB library was linked but not packed into the app. WebDAV still works. CI now refuses any build with that mistake.

- GameCube and Wii: they cannot be playable without JIT (see docs/HARD_SYSTEMS.md).
- Symbian / N-Gage: not started.
- Direct Google Drive, Dropbox and OneDrive logins: they need developer app ids only the owner can register. They do work through the Files picker.
- Camera and Amiibo do not reach a 3DS game, because the 3DS core cannot take them.
- DS games do not hear the real microphone; the blow button stands in for it.
- Online play over the internet needs port 55435 opened on the host's router.
- The libretro iOS buildbot DOES carry azahar, flycast, ppsspp and dolphin (re-checked 5 October 2026; an earlier note here said it did not, which was wrong). Continuum still compiles azahar, flycast and ppsspp itself in CI so it controls how they are built (the buildbot's flycast, for one, will not run without JIT). Dolphin is not built (docs/HARD_SYSTEMS.md).


## The road to the rest of the systems

All twelve steps of the sequence in
[docs/SET_HW_RENDER_DESIGN.md](docs/SET_HW_RENDER_DESIGN.md) section 13, not just as far as N64.
Shown whole because stopping the table at step 6 hid the fact that **the Switch is on this list**,
at steps 10 to 12, and that it is a system to be emulated rather than a device to run on.

The DS is not on here at all, and that is why it shipped first: melonDS is software rendered on
iOS, so it needed none of this.

| Step | State |
| --- | --- |
| 1. wgpu owns the one `MTLDevice` | **Done** |
| 2. Composite pass generalised to N screens | **Done**, with 7 layout tests and 3 shader tests |
| 3. MoltenVK in-process, a triangle into an `MTLTexture` | **Done** on build 89 — device diagnostics `Vulkan triangle: OK` (MoltenVK → `MTLTexture` → wgpu zero-copy). Build 82 had failed on a forced portability instance extension; filter in `64df7c4` / IPA [Continuum-89](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-89-dacb3d4/Continuum-89.ipa) |
| 4. `SET_HW_RENDER` for Vulkan, against Beetle PSX HW | **Partial**. Frontend contract + live MoltenVK install path on `master`: after Metal attach, `prepare_vulkan_hw` creates a shared MoltenVK `VkInstance`/`VkDevice`/`VkQueue` (same extension filter as step 3); `SET_HW_RENDER` accept (or prepare if accept already happened) calls `install_vulkan_handles` and the core's `context_reset`. `set_image` records `VkImage` from `create_info`; tick tries `VK_EXT_metal_objects` → `adopt_frame_texture`. Host tests cover callback layout, interface field order, accept/refuse, `set_image`, and install→`context_reset`. **Beetle PSX HW is in `IOS_CORES` / `ios-all`** and in the IPA; Settings → PlayStation core → Beetle PSX HW selects it (PCSX ReARMed remains default). Launch now refuses without a user-supplied BIOS (no HLE, no fake BIOS) and the BIOS line names Beetle honestly. The game page "Runs on" line and the Library detail follow that same choice. Telemetry HUD shows `HW` / status “Beetle HW: first Vulkan frame…” when `hardwareFrame` flips — **that has not been seen on a phone yet**. Build 98 device, after `scph1001.bin` was present: Crash booted on the Beetle core (~60 fps, frames ~2900) while hardware-render still read `installed=false`. That line is “handles not handed over yet,” not a black screen — the frames were on screen. Those frames do **not** prove the picture went through the Vulkan handoff. Core boot is real. Do not stamp step 4 Done just because the game moved, and do not treat `installed=false` as proof nothing drew. Brett said that boot is enough to move on to 3DS; do not reopen it in chat. iOS core builds remain Mac/CI-only |
| 5. Recompiler measurement | **Not started**, and blocked: no JIT on the owner's install |
| 6. **paraLLEl-N64**, the N64 | **Software core is Done** on build 97 (Smash character select, ~60 fps, no JIT). This row used to say Not started. That was the Vulkan RDP and the recompiler, which are still **not started**. Do not read “not started” as “N64 is not in the app.” |
| 7. ANGLE alongside MoltenVK | **Partial**. OpenGL and OpenGL ES `SET_HW_RENDER` are accepted. They used to be refused; that was an app choice, not the phone. Preferred answer stays Vulkan, so Azahar and Beetle still take MoltenVK. On iOS the accept path creates an EAGL OpenGL ES context and an FBO, `get_current_framebuffer` returns that FBO, and `get_proc_address` is `dlsym` into OpenGLES. A hardware frame is read with `glReadPixels` into top-left RGBA8 and uploaded by the compositor that already shows software frames. Host tests prove the accept, the callbacks, that Direct3D is still refused, that a Vulkan request still wins, and that a bottom-left GL buffer becomes the exact RGBA the upload passes through. **Not device-proven.** No phone has shown this frame. **ANGLE is not in this build**: no Metal client-buffer, no zero-copy `MTLTexture` from GL. The copy is the gap. Desktop `OPENGL` / `OPENGL_CORE` are accepted so the core is not refused, but the context is OpenGL ES, because iOS has no desktop GL. A core that needs a desktop-only entry point will not draw. If ES3 cannot be created, ES2 is tried, and that ES2 context has depth but not stencil. Do not stamp step 7 Done |
| 8. **Azahar** (Citra fork), the 3DS | **Partial**. In `IOS_CORES` and the IPA as `azahar_libretro_ios.dylib`. **Device:** Mario Kart runs. Apple builds have OpenGL off and Vulkan on; this core presents with `set_image`. CPU JIT and shader JIT are compiled out by `-DIOS`. No dynarec, no executable memory. The hitch this IPA changes is in Azahar's Vulkan rasterizer: `async_shader_compilation` defaults off, so every new pipeline sets `wait_built` and `BindPipeline` calls `WaitDone` on the emulation thread. The Continuum patch skips that accelerated draw until the pipeline worker finishes. The software-vertex fallback still waits, because that caller ignores a failed bind and would record a draw with no pipeline. That is the smallest wait the source proves. It has **not** been tried on a phone, so do not call the stutter fixed. A retail title may need system archives the app does not ship, and `SET_MESSAGE` is ignored |
| 9. **PPSSPP**, the PSP | **Partial**. In `IOS_CORES` and the IPA as `ppsspp_libretro_ios.dylib`. The iOS job compiles it for arm64. CPU is the IR interpreter (`IRJit` with compile-to-native false). No JIT, no dynarec, no executable memory. Vulkan `set_image`. No BIOS. `.cso` only. **Not device-proven.** The App Store PPSSPP build is also interpreter-only; lighter games are reported smooth there and heavy ones hitch. That is not this phone. Do not call it Done until a game runs on Brett's phone |
| 10. **Switch**, stage 1: the wrapper against a stub engine | **Partial**. `native/switch-wrapper/` already has the `retro_*` skeleton, the frame gate, a Vulkan stub renderer and a test harness. No Rust engine behind it |
| 11. Switch, stage 2: a real engine behind `ISwitchEngine`, homebrew booting | **Not started** |
| 12. Switch, stage 3: capability shim, shader cache, retail content | **Not started** |

### The Switch, since it is the one that gets misread

**It is the last and hardest SYSTEM TO EMULATE, after N64. It is not a platform Continuum runs on.**
Section 12 of the design document is the whole approach: every existing Switch engine is a
standalone application rather than a plugin, so `continuum_switch_libretro.cpp` wraps one behind an
`ISwitchEngine` interface and hands it to the rest of Continuum as an ordinary libretro core, which
is why nothing else in the engine has to learn that anything unusual happened.

Section 11 of that document also establishes it fits in memory, with the entitlements, on 12 GB
hardware and only there: roughly 4 GB of guest RAM plus 1 to 2 GB of GPU resources plus recompiled
code, against a 6 to 8 GB working budget.

### The recompiler is separate from the N64 that already runs

**Smash already ran without a JIT.** Build 97 reached the character select at about 60 fps on the software core. A recompiler is not a gate on N64 existing. It is still **not in this signed app**. What follows is only how a future recompiler would be allowed, not a claim that the current N64 cannot run.

iOS does not gate executable memory on the `com.apple.security.cs.*` keys in
`Continuum.entitlements`; those are macOS hardened-runtime keys and iOS ignores them. Nor does it
require `dynamic-codesigning`, the TrollStore route, which no provisioning profile can carry.

**What it actually takes is `get-task-allow` plus an attached debugger.** A debugged process gets
`CS_DEBUGGED` and the kernel permits executable memory. StikDebug and StikJIT do the attaching
entirely on-device over a local VPN loopback, needing a computer only once to make a pairing file.
`get-task-allow` is grantable, but ONLY by a development provisioning profile, so **signing this app
with a distribution identity silently rules out every recompiler system.** The app now reports
whether its own installed copy has the entitlement, so this is checkable rather than guessed.

One more piece is required on this hardware. Where TXM/SPTM is present, which it is on recent
devices, attaching is not sufficient: each executable region has to be prepared through the debug
connection first, using a breakpoint protocol the HOST APP must implement:

    JIT26Detach()                      mov x16, #0 ; brk #0xf00d ; ret
    JIT26PrepareRegion(address, size)  mov x16, #1 ; brk #0xf00d ; ret

Order matters and a `brk` with no script attached crashes the process, so this is gated work rather
than a flag. It is Part 1 of StikJIT's integration guide and is **not started**. That does not block the software N64, which already runs. Do not put the old line back that N64 cannot run without a JIT.

Reading the cores established the rest: `pcsx_rearmed` and `mupen64plus-next` have **no** Apple JIT
support at all, so switching the PlayStation recompiler on would have failed at the first executable
page and looked like a broken core. `parallel-n64` **does** have it, and it is also the core the
graphics design already chose for unrelated reasons. Its iOS build disables the recompiler, so
enabling it means porting that core's macOS arm64 configuration.

**MoltenVK in the bundle**, roughly 8 MB on an app that is currently 7.3 MB. Unavoidable:
paraLLEl-RDP is Vulkan compute and has no GL equivalent.

---

## Known problems

- **The app would not open, and it was the JIT probe.** Fixed. That probe writes a function into
  memory and calls it, and it ran at STARTUP. Executing a page the process just wrote is the one
  thing iOS terminates an app for unless the dynamic-codesigning entitlement is genuinely in force,
  and whether it is depends on how the copy was signed and installed rather than on the build:
  TrollStore preserves it, a free developer account does not. So on those installs the app opened
  and was killed before drawing anything, including the line the probe existed to print. **That
  silently blocked every on-device test for several builds**, which is also why the JIT line never
  came back. Startup now only asks whether such a page can be MAPPED, which is a real answer and
  cannot get the process killed; the half that runs code is a clearly labelled button in Settings.

- ~~**mgba is flaky in CI.**~~ **Diagnosed and fixed.** The archive holds LLVM bitcode, because
  mgba's CMake adds `-flto` unconditionally on Apple through a condition that parses as
  `APPLE OR (GNU AND BUILD_LTO)`, so `-DBUILD_LTO=OFF` could never have turned it off.
  `-force_load` guaranteed the archive members were loaded and then LTO decided what to emit from
  them, and since nothing in the link referenced the libretro API, LTO was free to internalise it.
  Whether it did came down to how its parallel codegen partitioned the module, which is why the
  same commit passed and failed on different days. Every entry point is now named with `-u`, which
  makes it a root before LTO runs. One clean build so far, and the mechanism is deterministic
  rather than lucky.
- **The staged-dylib check now covers all 20 entry points**, not just `retro_run`. One symbol only
  ever caught a core that resolved nothing; a core that loses one entry point is both likelier and
  much quieter. That is not hypothetical: save states were broken for this project's whole life
  because `retro_serialize` was never resolved.
- **`swiftc` on Linux cannot type-check.** It is a syntax pass, so a Swift type error, an actor
  isolation mistake or a wrong argument label is only caught by CI. Several builds have been spent
  on exactly that, and it is a property of the toolchain rather than carelessness.
- **The core sources are pinned (build 122).** Every emulator built from source is checked out at the
  exact commit build 121 was built from (`IOS_PIN` in `scripts/build-core.sh`), and the 19
  downloaded ones must match build 121's checksums (`scripts/fetch-buildbot-cores.sh`, with a frozen
  copy on the `core-mirror-1` pre-release). Before this, every build took whatever each upstream had
  that day, and parallel_n64's upstream had already moved since build 121. Moving a pin is a
  deliberate step, written down in `build-core.sh`. `core-sources.txt` in each build's metadata still
  records exactly what was built.

---

## Deliberately deferred

Not forgotten, and not bugs:

- iOS deployment target stays at 16 rather than 18, and Swift stays at language mode 5.
- Separate saved control layouts per orientation for the built-in pad (no imported skin). It does re-arrange itself for landscape. Imported skins swap portrait and landscape.
- **PSP (PPSSPP)** is step 9 and is **in this IPA as Partial**, not Done. The CPU is the IR interpreter, no JIT and no dynarec. It has not been tried on a phone. App Store PPSSPP is also interpreter-only; lighter games are reported smooth there and heavy games hitch. That report is not a Continuum result. Nothing to do with Switch is deferred; it is steps 10 to 12. 3DS is in the app as Partial (a game runs, then stutters), not deferred.
