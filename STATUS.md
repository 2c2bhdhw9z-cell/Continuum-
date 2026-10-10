# What is finished, and what is not

One page, kept current, so nothing has to be inferred from a commit log. Last updated
10 October 2026. Newest published install: see the top of the build notes below (build 163 was
[Continuum.ipa](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-163-676af12/Continuum.ipa)). The paragraph below about build 153 is older history.
It has everything from build 152 (the second 3DS stutter fix and Continuum Symbian, whose
`continuum_symbian_libretro_ios.dylib` is in the IPA) plus Mesen 2 as the default NES core. The
next build after it adds the beta-readiness pass (open-source credits in Settings → About, the SMB
wording in the Local Network permission). Older build notes further down are history; where an
older line here disagrees with this paragraph, this paragraph is the current one.

**Build 164 (next, built, untested on a phone):**
- **Saving outside the app: real cause of the dead Open button.** Builds 161 and 163: the folder picker opened, Open highlighted, nothing happened. Continuum is sideloaded and re-signed, and iOS does not give a re-signed app the File Provider grant an open-in-place folder pick (`asCopy: false`) needs; the same "Open does nothing" is reported for other sideloaded apps, and every other picker in Continuum (all `asCopy: true`) works on Brett's phone. Build 163's SwiftUI `.fileImporter` swap could not help (and the repo had already found `.fileImporter` drops its completion on device). Now Settings → SYNC FOLDER has **Back up to Files** (syncs into a folder inside the app, then exports a copy with the Files exporter) and **Restore from a backup** (copy-mode pick of that folder, merged in, nothing deleted). The backup prompt runs Back up to Files. The live folder stays as "Live folder (advanced)". Every picker step (presented, on screen, delegate called with URLs, cancelled, dismissed, security scope, bookmark) is in the activity log, and errors show in the Settings line. Confidence: high on the cause, medium that the folder exporter works on his iOS version. Backups are manual, not automatic. TESTING P-SYNC2.

**Build 163 (published 10:58 AM PT 10 Oct; on the phone the folder picker was still broken, PSP not yet reported):**
- **PSP gray screens / lone triangle / tiny thumbnails (NFS, Midnight Club), likely cause found.** Build 161 on the phone: NFS reaches its menus, but loading screens showed two gray bars, a black screen with one small triangle after the EA logo, and Midnight Club a gray screen with two white lines and, after "Continue without saving", three tiny thumbnails. The engine declared PPSSPP's image to the compositor at the frame size from video_refresh even when the real image was bigger; the shader samples with 0..1 coordinates measured against the REAL image, so a smaller frame was squeezed into a corner and the rest of the screen showed leftover image contents. Software rendering showing the same thing fits: it is the frontend, not the GPU path. Now the image is wrapped at its real size and only the frame's rect is copied out and shown. Confidence medium: the activity log now has a `hw frame size: shown WxH, image WxH [CROPPED]` line on every change, so a report from this build proves or disproves it. TESTING P-PSP4.
- **Sync Folder picker.** Open did nothing in Settings' folder picker on build 161 (picker never closed). It was a UIKit picker presented by hand from the topmost view controller over SwiftUI; it is now SwiftUI's own `.fileImporter` for folders, in Settings and in the backup prompt. "Cloud Sync" is now called "Sync Folder" in the app: it is a folder on the phone (or any Files location). Confidence medium: the old code looked correct on paper, so the cause was not proven.

**After build 158 (built, untested on a phone):**
- **PSP picture and resume.** Build 158 opened PSP games on the phone (crash fixed), but the picture
  was a half-drawn gray ghost, then stuck, and a resumed auto-save stayed black. Causes found in the
  engine: PPSSPP submits its GPU work with no semaphores and relies on the frontend to wait for it
  (`wait_sync_index`, which was a no-op) and to not read the image while it is drawing; the engine
  read it right away and gave it only one image to draw into. For PPSSPP only, the engine now waits
  for the core's queue before showing a frame, gives it two images that alternate, and makes
  `wait_sync_index` really wait. `ppsspp_inflight_frames` defaults to "No buffer". Image format is
  read from the core (R8G8B8A8 vs B8G8R8A8) for every Vulkan core; Azahar's is unchanged. A PSP
  auto-save is resumed after ~30 frames instead of before the first one, when PPSSPP has no GPU yet.
  The NFS static is not explained; it may be the stalled game. TESTING P-PSP2.
  Build 161: the gray ghost showed with PPSSPP's software renderer too, so it was not GPU sync. PPSSPP was built with USE_FFMPEG=OFF, so PSMF movies (EA loading screens) and their audio never decoded: ghost + buzz. Now USE_FFMPEG=ON (ffmpeg/ios/universal). GPU path unchanged (Vulkan). Adopted frame clamped to the real MTLTexture size; crash report body carries the last 50 log lines with PSP `frame:` phase markers. TESTING P-PSP3.

**After build 157 (built, untested on a phone):**
- **PSP crash on opening a game, root cause found.** PPSSPP asks for its Vulkan context from inside
  `retro_load_game`, before its own context pointer is set, and the engine called `context_reset`
  right then, a call through a null pointer. PPSSPP also makes its own `VkDevice` through the
  negotiation interface's `create_device`, which the engine stored but never called. Now the reset
  waits until load returns (PPSSPP only), the core's `create_device` is called on the shared
  MoltenVK instance, and its images are exported through its own device. If that setup fails the
  game does not open and the reason is shown, instead of a crash. TESTING P-PSP.
- **3DS shader switch.** Core settings for the 3DS now has **Shader Compile (Continuum)**:
  "Async shaders (fix, may pop in)" (default, the build 152 fix) or "Old (may stutter)". Takes
  effect on game restart. TESTING P-3DS.
- **Launch crumbs.** Before each launch step (game file, system and core; core load; retro_init;
  retro_load_game; HW context; first frame) a line is written and fsynced to the activity log, so
  the next crash report names the step that died.

Five states only:

- **Done** means built, in the `.ipa`, and a phone showed it working.
- **Partial** means part of it is confirmed or built and a named piece is missing or not yet on a
  phone. The note says which part is confirmed.
- **Built, untested** means it is in the app and should work, and nobody has tried it on a phone
  yet. Those are the rows in [TESTING.md](TESTING.md)'s queue.
- **Not built** means it is not in the app.
- **Out on purpose** means it is left out deliberately.

Nothing here is rounded up.

For what the owner wants built, and the scope rules this page works inside, see
[docs/PRODUCT_SCOPE.md](docs/PRODUCT_SCOPE.md).

## Public beta readiness (audit, 9 October 2026)

What a code pass on the whole app found. Rust tests: 614 pass (`cargo test --workspace
--features emulator-bridge/native-core`). The Swift checks (`check-players.sh`, `check-skins.sh`)
need `swiftc` and only run on the Mac CI runner.

Already fine: no `try!` or forced casts that can fire at run time (the only `fatalError`s are the
storyboard initialisers UIKit never calls), the skin unzip has a 64 MB per-file cap, the backup
prompt is not shown on a first launch with an empty library and waits for the crash prompt, the docs/BUG_AUDIT.md items spot-checked (1, 11, 12, 32) are fixed in the code (the rest were
not re-checked), Camera, Microphone and
Local Network permission texts are present.

Fixed in this pass: an open-source credits list (each shipped core and library, its license, a
link to its source) under Settings → About; the Local Network text now mentions SMB; a Settings
note that quoted an internal ticket number ("FEAT-006") was reworded.

**What still blocks or limits a public beta (licensing):**

- **Non-commercial cores:** Snes9x, Genesis Plus GX, PicoDrive, FinalBurn Neo and MAME 2003-Plus
  may only be given away free. A free beta by direct IPA link is fine. Any paid version, ads,
  tips-for-access, or App Store sale is not allowed with them in the app.
- **GPL cores** (most of the rest, including Mesen 2, melonDS, Azahar, PPSSPP, and EKA2L1 inside
  Continuum Symbian, which is GPL v3): anyone given the IPA must be able to get the source. The
  repository is public, which covers it as long as it stays public and each release's commit is
  kept. The repository has **no license file of its own**. The owner should choose one; because
  Continuum Symbian links GPL v3 code, GPL v3 is the simple, compatible choice. That is the owner's
  decision, not something changed here.
- **The App Store** is not an option with GPL cores (Apple's terms conflict with GPL) and the
  non-commercial ones. Sideloading only. TestFlight is also Apple
  distribution, so the same caution applies.
- Games, BIOS and phone firmware are never included, and must stay that way.

Known limits worth telling testers: audio uses a simple linear resampler (can sound slightly
harsh on some systems); Symbian sound ignores the app volume and has no save states.

**Second pass (build after 154):** all 39 docs/BUG_AUDIT.md items re-checked (table at the top
of that file): 31 already fixed, 7 fixed now (achievements retry after an offline launch, the sync
safety check per folder, exact local times in sync, a crash-proof picture conversion, Settings
text plus a confirm before Reset deletes every skin, Flash/J2ME pause while loading, SMB buffer
bounds), 1 still present (landscape-only skins), 1 needs a phone. GPL v3 LICENSE added; README
names it. None of these are tried on a phone yet.

## Line-by-line review: complete (9 October 2026)

Every file the project maintains was read line by line (log: [docs/REVIEW_COVERAGE.md](docs/REVIEW_COVERAGE.md)):
Continuum Symbian and the core patches, all app Swift (~44,000 lines), the Rust engine (~44,500
lines) and the scripts, build tooling and CI workflows (~6,500 lines). Vendored upstream core code
was not in scope. About 80 fixes in total; the ones that matter most:

- **Data safety:** one damaged cheat no longer empties and then overwrites the whole cheat list
  (the original is backed up first); "Delete every save state / stored cheat", the per-game
  save-state and cheat trash buttons and the in-game cheat delete all ask first; an auto-save
  deleted while paused is written again; sync refuses a run that would suddenly empty a folder.
- **Crashes and memory:** SMB error and size handling bounded; a bad frame shows black instead of
  crashing; duplicate Flash/J2ME keys no longer crash; cheat RAM search stays inside a resized
  memory block; the Vulkan path (3DS, PSP) no longer frees memory the core still uses, reports the
  real graphics queue, and frees a failed setup; a bad `.iso`, NaN speed/timestamps, extreme sync
  timestamps and bad stick or touch values are all handled.
- **Behaviour:** achievements log in again after an offline launch; a held fast-forward cannot
  stick after the layout editor; disc games set to another system get the right skins; Dreamcast
  `.bin` saves import; odd-sized N64 saves convert correctly; core settings that fail to save are
  not applied; Symbian opens on a clean picture and edge taps land on screen.
- **Scripts/CI (last area):** the weekly core-update page no longer says "all up to date" when
  repositories could not be reached; a player that fails to download no longer keeps a stale
  provenance line; several stale build comments fixed. No CI behaviour changed.

Known and left on purpose: landscape-only skins are saved as upright ones (to be fixed with a
phone test); two games with the same file name share a star and "added" date; a Symbian package
install failure is not shown on screen; a new cover of exactly the old file size does not sync.

**Still untested on a phone** (all built, none confirmed): every fix from this review (builds
155, 156 and the next one); the 3DS stutter fix; Continuum Symbian; PSP; SMB file sharing; the 3DS
camera and Amiibo; Game Boy, Master System, FDS and SG-1000; the open-source credits page; a first
launch with an empty library.

## Next up

For whoever works on this next:

- **Build 150 is the newest published IPA** (`ef3ce42`).
  [Continuum.ipa](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-150-ef3ce42/Continuum.ipa).
  Same app as 149. Continuum Symbian is not in it.
- **Build 149** (`d2a92a9`) added SMB (libsmb2 compiled into the app, not a framework), the 3DS
  camera, and an Amiibo tap. None of those three have been tried on a phone. Builds 147 and 148
  died in `smb_min.c` and published nothing. The cores from 147 were cached and reused.
- **Build 146** (`efd798e`) is the previous install. The icon grid marks the icon that was
  actually set. On a sideload, iOS can leave `alternateIconName` empty even after the home screen
  changed, so the grid remembers the name it passed to `setAlternateIconName`. Every Settings card
  folds. Open all / Close all is on the Settings title.
- **Build 145** (`1a28e84`) added the Settings switcher for 55 home screen icons.
- Feedback already goes to idkplswrk@gmail.com (`FeedbackDestination.email` in
  `native/ios/Feedback.swift`). Mail opens addressed to it when Mail is set up. Otherwise the share
  sheet opens and the text starts with that address. There is no missing address. An older line on
  this page said there was. That line was wrong.
- Symbian / N-Gage: **Built, untested** in the next build (not in 150). Continuum Symbian
  (`native/continuum-symbian`) is now linked to EKA2L1's jitless iOS port
  (MuhannadYT/EKA2L1_IOS, pinned), on its dyncom interpreter, with no JIT. It is an OPTIONAL core,
  `continuum_symbian_libretro_ios.dylib`: if it does not compile, the IPA ships without it. `.sis`,
  `.sisx` and `.n-gage` files now go to a Symbian system with a phone-keypad pad. No phone has
  run it. Missing or unknown: whether it compiles on CI at all (nobody can build it off the Mac
  runner), whether frames read back from the off-screen layer reach the screen, sound goes
  straight to the speaker (not through the app's volume, rewind or recording), save states do
  not work, there is no core-settings page, and you need your own firmware (TESTING.md S1). The
  only other core this project wrote is the Switch one, `native/switch-wrapper`, which does not
  run games. A core somebody else wrote keeps their name (mGBA, Azahar). See
  [docs/HARD_SYSTEMS.md](docs/HARD_SYSTEMS.md).
- QA Wolf native iPhone testing is connected in the repository: it accepts the released `.ipa`
  directly (not a `.app`). Finish the three private/account steps: sign in to the QA Wolf MCP
  connection, ask QA Wolf to enable mobile triggers for this workspace, and store the API key as
  GitHub Actions secret `QAWOLF_API_KEY`. Then run **QA Wolf mobile** manually to upload a current
  build.
- Open work from [docs/MANIC_PARITY.md](docs/MANIC_PARITY.md) "Still to do": direct Google Drive,
  Dropbox and OneDrive logins (they need app ids only the owner can register; they already work
  through Files). SMB is no longer on that list.
- Phone results that decide the next work: Dreamcast and PSP speed (TESTING C19, C20), whether
  the 3DS stutter after transitions is gone with the second fix (TESTING A7), and whether the new camera and Amiibo path actually
  reach a game.
- Later, in this order: Symbian / N-Gage (built, waiting on a phone, TESTING.md S1), the Switch (road steps
  10 to 12), Android.
- melonDS JIT is not built: its Apple code is macOS-only (RWX `MAP_JIT` pages,
  `pthread_jit_write_protect_np`) and its fast-memory setup uses `shm_open`. DS runs full speed
  on the interpreter, so it waits.
- Build 144, phone checked: Home stays readable after you scroll past the big cover, Home and All
  Games scroll, re-importing a game keeps it and it still opens, fast-forward lets go when Control
  Centre takes the touch, and the (i) line names the phone. Still not shown: rewind letting go the
  same way, covers not flashing again, the phone name on the feedback screen, and the skin-list
  protections.

---

## Systems

**32 cores** are in the IPA in builds 119 to 124 (checked in the build 124 file itself: version
0.8.0 (124), 32 core files, every emulator at its pinned version). **38 systems** in all, run by
the 32 cores plus the bundled Flash and J2ME players (some cores run more than one system). The
table below is the first seventeen systems; the 3 October batch below adds the rest. The Switch
is still ahead, hardest last (road steps 10 to 12).

| System | Core | State | What is missing |
| --- | --- | --- | --- |
| NES | mesen2 (default), fceumm (fallback) | **Done** | Mesen 2 confirmed on the owner's phone, build 153 (9 October 2026): AccuracyCoin.nes **142 of 146 passed, 0 skipped** (FCEUmm baseline on the phone: 86 of 146, 7 skipped). Save state slot 1 saved and loaded on Super Mario Bros 2 (Lost Levels). FCEUmm stays selectable in Settings, NES core. Save states from one core do not load on the other. Battery save on Mesen not yet seen on a phone |
| SNES | snes9x | **Done** | |
| Game Boy | mgba | **Done** | Build 125: a game ran, saved and loaded |
| Game Boy Color | mgba | **Done** | |
| Game Boy Advance | mgba | **Done** | |
| Master System | genesis_plus_gx | **Built, untested** | No `.sms` file has ever been imported. Game Gear and Mega Drive on the same core are confirmed |
| Game Gear | genesis_plus_gx | **Done** | |
| Mega Drive / Genesis | genesis_plus_gx | **Done** | |
| PlayStation | pcsx_rearmed | **Done** | Interpreter, not the recompiler. Fast enough; see Recompiler below. Runs without a BIOS. **Beetle PSX HW** is a second PlayStation option (Settings → PlayStation core) and needs a real BIOS file. Beetle booted Crash at ~60 fps on build 98 (with `scph1001.bin`) on its software renderer; the engine now keeps Beetle on software by default. The hardware (Vulkan) renderer is a choice in Core settings and has not been seen on a phone. Since build 124 the BIOS line reads `BIOS (mednafen_psx_hw)` with Beetle selected and finds BIOS names in capitals (TESTING A7). A save state from one PlayStation option does not load on the other. See road step 4 |
| **Famicom Disk System** | fceumm (default), mesen2 (choice) | **Built, untested** | Mesen 2 is a Settings choice here, not the default, because the app's disk-side button drives FCEUmm only; Mesen inserts disks on its own. Needs `disksys.rom`, which is Nintendo's own code and cannot ship with the app. The launch path checks for it by name and says so rather than letting the core fail |
| **Sega SG-1000** | genesis_plus_gx | **Built, untested** | Needs nothing extra |
| **Nintendo 64** | parallel_n64 | **Done** | Device-proven on build 97 (`258a828`): past `N64 first tick…`, frames climbing, ~60 fps into Smash character select. Soft/interp only (no JIT on this signed IPA). `.n64`, `.z64`, `.v64` |
| **TurboGrafx-16** | mednafen_pce_fast | **Done** | HuCard games (`.pce`): a game ran, saved and loaded on build 125. PC Engine CD is its own system (Beetle PCE) and needs a system card BIOS you supply (TESTING C15) |
| **Atari 2600** | stella2023 | **Done** | A game ran, saved and loaded on build 125. `.a26`, or a lone `.bin` (the app asks **Which system?** once; renaming to `.a26` skips the question). A `.bin` imported beside a `.cue` is always a disc track |
| **Nintendo 3DS** | azahar | **Partial** | A game runs (Mario Kart 7, again on build 122). Confirmed 5 October (build 122): skin holes sideways (Mario Kart 7), and changing a "restart required" setting no longer crashes. Upright skin holes and swapping them confirmed 9 October (build 150 screenshots). A stutter after transitions was seen on builds 100 and 101; a fix has been in every build since 2 October and Mario Kart 7 ran on 122, and on 9 October (build 150) Brett confirmed the stutter is **still there**. Open bug. A second fix is built (9 October, after build 150) and **not tried on a phone**: the first fix stopped one wait, but Azahar still asked the graphics driver for each new pipeline on the game's own thread first, and on iPhone that request does the slow Metal compile right there. The new patch sends every new pipeline to the background worker, and the software-vertex path now skips the draw instead of waiting. No JIT. Decrypted `.3ds`, `.3dsx`, `.cci`, `.cxi` only. A retail game can still need 3DS system archives this app does not ship |
| **Nintendo DS** | melonDS | **Done** | Confirmed on device (build 80): Mario Kart DS and Pokémon SoulSilver, dual screens live, ~60 fps, 0 dropped |
| **PlayStation Portable** | ppsspp | **Partial** | In the IPA as `ppsspp_libretro_ios.dylib`. CPU is the IR interpreter: the core's option value "IR JIT" is `CPUCore::IR_INTERPRETER` with compile-to-native off. No dynarec, no executable memory. Picture is Vulkan `set_image`, the same hook as the 3DS. No BIOS is shipped; PPSSPP does not need one. A PSP game can be `.cso`, `.iso`, `.chd`, a PSP `EBOOT.PBP`, or `.prx`. `.cso` and `.prx` are always PSP. An `.iso`, `.chd` or `.pbp` is looked inside: a PSP disc or EBOOT opens on the PSP, a PlayStation one as PlayStation, and if the app cannot tell it asks **Which system?** once and remembers. `.elf` is not accepted. **Not tried on a phone.** Do not claim a game runs or quote a frame rate |

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
| BIOS | **Done**, no files needed. This build carries a FreeBIOS and generates a firmware when the dumps are absent. The three names stay declared so Settings still reports what it finds, and none is a fine answer |
| Speed | **Done for the titles tried**. Mario Kart DS and SoulSilver held ~60 fps with 0 dropped on build 80 (software rendered, single threaded). Threaded renderer remains a lever if a heavier title is slow |

---

## Features

| Feature | State | What is missing |
| --- | --- | --- |
| Import, including multi-file `.cue` plus `.bin` | **Done** | |
| Library, cover art, the detail card | **Done** | |
| Cover art from the internet | **Done** | |
| Cover art from your own file | **Done** | |
| Cover art captured from the running game | **Done** | Confirmed on the phone with build 122. Build 123 makes it a sharp picture of the game alone (no skin layout around it); confirmed on build 125 |
| On-screen controls | **Done** | |
| Physical controllers | **Done** | Including a controller and thumbs at the same time, and (build 125) hiding the on-screen pad while a controller is connected |
| Sound | **Done** | |
| Volume and mute | **Done** | |
| Screen fit and scaling | **Done** | |
| Fast forward | **Done** | 2x (build 122), 3x, 4x and slow motion (build 125) confirmed. Tops out near 4x. Past that the engine drops frames instead of going faster, and the menu stops at 4x. That is not 5x |
| Rewind | **Done** | |
| Save states, slots, delete | **Partial** | 50 fixed slots per game plus the auto-save, each with a picture, date and core; export and import of a state file and of the game's own battery save (`.srm`). Old numbered saves move into free slots and nothing is deleted. Confirmed: Save to the next free slot (build 122); on build 125, slot pictures, save and load on 14 systems (NES, SNES, Game Boy, GBC, GBA, Game Gear, Mega Drive, PS1, DS, TurboGrafx-16, Atari 2600, N64, Jaguar, Pokemon Mini), exporting a 3DS state to Files and importing it back as a new slot, rename, save over, delete, and an exported and imported state loading on a non-3DS game. Build 126, confirmed on the phone: 3DS states load (they were all refused as "too short" on 125), and an imported state keeps its picture |
| Auto-save and resume | **Done** | |
| Cheats | **Partial** | Confirmed: typed codes (GameShark, Game Genie, Action Replay and the rest go straight to each system's emulator, which reads its own kinds). Build 126, confirmed: a typed code can be added inside a game, and both cheat screens say which kinds that system reads. **Bug found on 126, fixed in 127, not on a phone yet:** a switched-off cheat kept running on the GBA and Game Boy (mGBA ignores the off flag), so a code for the wrong version of a game froze it with no way out; the cheat screen now also names the exact game and version from the file (TESTING.md A2). Not on a phone: importing a RetroArch `.cht` file, and the RAM search (lives, money and so on) that turns an address into a cheat. Up to 128 codes per game. Build 122: a `.cht` file's RAM cheats land where RetroArch puts them (on GBA they used to hit the wrong memory) |
| On-screen control layout editor | **Partial** | **Done bar (Brett):** every control in the skin file works, not only the ones he names. Picture in the screen hole both ways you hold the phone. Two screens when the skin has two. A joystick or circle pad is a real stick, not a dead picture. Shoulders too. Debug text off the picture. Confirmed on the phone 5 October (build 122, a 3DS skin, Mario Kart 7): both screens sit in their own holes sideways, the eye button hides and shows the top bar (remembered), nothing sits on the picture of a sideways 3DS skin, the false overlap line is gone. Portrait and swapped holes on a 3DS skin confirmed 9 October (build 150). DS skin confirmed both ways 9 October (HeartGold). Other systems' skins not confirmed yet. Do **not** stamp Done until the owner says the skin is right |
| Battery saves (the game's own save) | **Done** | Confirmed on build 125: an in-game save is still there after leaving and coming back. Found broken while building the save manager: in-game saves (Pokemon, Zelda, PS1 memory card) were never written to disk, so they only survived inside a save state. Now restored before the first frame and written when you leave or switch apps |
| Save state compatibility refusal | **Partial** | Refuses a state from a different core or core build, and (since build 120) one saved under different restart-required core settings. Confirmed on the phone 5 October (build 122): changing a 3DS restart setting no longer closes the app. **Bug found on 126, fixed in 127, not on a phone yet:** changing the 3DS System Model mid-game crashed the app and showed no restart button (TESTING.md A1). Azahar re-reads every option on an update, so it switched models under the running game; the engine now holds a restart-required option until the next start, and every state is stamped with the value the game is really running with |
| Hide the player's top bar | **Done** | Build 122, confirmed on the phone 5 October: the eye button next to Back hides and shows the top bar, and the choice is remembered between games |
| Import several skins at once | **Done** | Build 122, confirmed on the phone 5 October (skin library, Import skins) |
| A paused game stays paused after leaving the app | **Done** | Build 122, confirmed on the phone 5 October |
| Wi-Fi transfer access code | **Built, untested** | Build 122. The address now ends in a short code that changes every time Wi-Fi transfer is switched on; anything without it gets nothing, so nobody else on the Wi-Fi can upload files or download saves. TESTING.md B1 |
| Feedback for testers | **Partial** | Goes by email to idkplswrk@gmail.com (`FeedbackDestination.email` in `native/ios/Feedback.swift`). Mail if Mail is set up; otherwise the share sheet, and the text starts with that address. Build 126's form was confirmed working and called "basic" by the owner. Build 127, not on a phone yet (TESTING.md A3, A4): from a game it opens on How it runs (a rating and what is wrong), the picture can be drawn on, every report attaches the activity log (every status line with its time, kept on disk so it survives a crash), the tester's name is remembered, and after the app closes by itself the next start offers a crash report. A save that was loading when the app closed is not loaded by itself again. An older line here said there was no address yet. That was wrong. The address has been in the source since build 129 |
| Apple performance overlay switch | **Done** | Confirmed on build 125. Build 121. Settings → DIAGNOSTICS. Hides Apple's Metal Performance HUD on the game layers and turns off the launch-time request for it. May need the app reopened |
| Landscape with no skin | **Done** | Froze in build 119 (an endless layout loop from a repeated warning line). Fixed in build 120 and confirmed on the phone |
| **+** opens Files, the ⋯ menu, TV picture quality | **Done** | Confirmed on build 119 |
| Honouring what a core wants its content as | **Done** | Every core used to be handed a file path and no bytes. That worked for the first six by luck, and would have given Stella a zero-byte ROM, because it copies straight from the data pointer with no path fallback. The engine now reads what each core declares and loads the file when the core wants bytes, so the next such core needs no change |
| File formats per system | **Done** | Every extension is now taken from the cores' own declared lists rather than a hand-written one. That added the two systems above plus `.smd`, `.swc`, `.fig`, `.unf`, `.unif`, `.sgb`, `.mdf` and `.toc`, which were being refused despite being supported |
| Core settings | **Partial** | Every core's options can be changed in **⋯ → Core settings…**. Confirmed: a 3DS restart-required change (build 122), and **Restart the game now** coming back inside the skin, starting from the beginning with auto-save off (build 125). Other cores not on a phone yet (TESTING.md F1). The engine pins only a few: melonDS starts with touch mode on and boots the game directly (both can be changed); parallel_n64's renderer is left to the core (it picks angrylion), with the HLE RSP, single-threaded angrylion and the cached interpreter CPU, all locked and hidden; PPSSPP's CPU is locked to the IR interpreter; Beetle PSX HW's renderer defaults to software (hardware can be picked) |
| Multi-screen compositor | **Done** | Drives the DS and 3DS layouts and the screen swap. Confirmed: DS stacked screens (since build 80), a 3DS skin's two holes sideways (5 October), and the swap button and the six layouts with touch landing where you tap (build 125) |
| Android `.apk` | **Not built** | The one other PLATFORM, and the only one after the iPhone. Still far off: it comes after Symbian / N-Gage and the Switch (Next up). Everything new goes in the Rust engine so Android inherits it |
| Switch wrapper (to EMULATE the Switch) | **Partial** | `native/switch-wrapper/` has the frame gate, a Vulkan stub and a test harness, with no engine behind it. Steps 10 to 12 of the road below |


## The other app's list

This is the bar. Same five states as the top of this page: a line is **Done** only when a phone showed it. **Built, untested** means it is in the app and no phone has run it. **Not built** means it is not in the app. JIT is **out on purpose**, not a missing feature.

Every row below is in the current install (newest on the Releases page). A skin saved on the phone before build 109 only has the first hole and no circle pad, so import the skin file again.

| What | State | Notes |
| --- | --- | --- |
| Two screens, each placed where the skin file says | **Partial** | Confirmed sideways on a 3DS skin: the owner's 5 October screenshot (Mario Kart 7, build 122) has both screens in their holes. Upright confirmed 9 October (build 150), and swapped holes too. DS skin confirmed upright and sideways 9 October (HeartGold, build 150) |
| Swap which screen is the big one | **Done** | Confirmed on build 125, with touch still landing where you tap. DS and 3DS. Six layouts in Settings (stacked, side by side, big top, big bottom, top only, bottom only) and a swap button in the player. Touch follows the bottom screen. With a two-hole skin the pictures trade holes |
| AirPlay: game on the TV, touch screen on the phone | **Done** | AirPlay or a cable. Game on the TV, controls on the phone. On DS and 3DS the phone keeps the touch screen. Two switches in Settings. Confirmed: the sharper TV picture (build 119), and the game on the TV with TV scaling and TV layout (build 125) |
| Button shows a pressed picture | **Built, untested** | Since build 109. Only when the skin file has that picture |
| Extra buttons you place yourself | **Built, untested** | In the layout editor: add a button, a combo, a turbo button, or an action (quick save, quick load, fast forward, rewind, screenshot, pause). Drag, resize, fade, delete. Per system and per way you hold the phone |
| Edit an imported skin inside the app | **Built, untested** | Move and resize every button, stick and screen hole, change what a button presses, fade the skin, reset to the file. The imported file is never changed |
| Circle pad or joystick as a real stick | **Built, untested** | Since build 109. Not a D-pad. Import the skin again only if the skin was imported before build 109 |
| Touch screen as a mouse | **Built, untested** | Per system in Settings. Drag moves, tap clicks, two fingers right click. Only games that support a mouse respond (Mario Paint, PlayStation mouse games) |
| iPhone camera into a 3DS game | **Built, untested** | The app can feed the camera (front or back, off until the Settings switch is on). Azahar is patched to ask for that raw-framebuffer camera and to use it for all three 3DS cameras. Not on a phone yet. The switch still has to be on, and iOS still has to allow the camera |
| iPhone microphone | **Built, untested** | 3DS games that listen (Azahar asks for it). Switch in Settings, off by default. DS games do not use it: melonDS only fakes a blow on its L2 button |
| Amiibo file | **Built, untested** | Import and pick Amiibo files in the 3DS ... menu. A tap writes the dump and a stamp into the core's save directory. Azahar loads it with its own `LoadAmiibo` when the game is looking for a tag, and keeps the tap until then. Not on a phone yet |
| Haptics on a button press | **Done** | Button taps and game rumble confirmed on build 125. Off, light, medium or strong in Settings. Also game rumble on the phone and on controllers, with its own switch |
| JIT | **Built, untested** | Build 133 (part 2).  Used by itself when JIT is on: PSP and 3DS from their regular builds, PlayStation, N64 and Dreamcast from second `_jit_` builds. Not on iOS 26 iPhones with TXM (A15 and newer) yet: they need StikJIT's region protocol. The owner's phone cannot use JIT, so a tester has to confirm it |
| Rewind | **Done** | |
| Fast forward | **Done** | About 4x, not 5x |
| Save slots, including export | **Partial** | 50 slots plus the auto-save, export and import of states and battery saves. Save to the next free slot confirmed (122); slot pictures, save and load on 14 systems, export and import, rename, save over and delete confirmed (125). 3DS states loading and imported slots showing their picture confirmed (126) |
| Cheats: search, and importing a file | **Partial** | Typed codes confirmed; `.cht` import and RAM search not on a phone |
| Online play | **Built, untested** | Two phones, same game. Host or join on the same Wi-Fi (nearby list) or by address. Over the internet the host must open TCP port 55435. No rollback, so lag shows as short stalls. Rewind, fast forward and loading states are off while online. Build 122: if player 2 drops out, the host keeps playing and lets them rejoin on the same address |
| Achievements | **Partial** | RetroAchievements login, unlock banners and a list on the game card. Game Boy Advance achievements read mGBA's full memory map. Confirmed (build 125): logging in (the button says Logging in... and cannot be pressed twice) and the unlock banner popping up in a game. The list on the game card is not confirmed yet |
| Sync Folder (was "Cloud sync") | **Built, untested** | Pick any folder in Files (iCloud Drive, Google Drive, Dropbox) once. Save states, battery saves, cheats, settings and covers sync both ways; since build 122 also Flash and J2ME saves, PDF manuals, Amiibo files, the remembered "which system is this" answers and saved servers (their passwords stay on each phone). Kept per phone on purpose: skins, the RetroAchievements login, favourites, the last online-play address, mic and camera permission. Conflicts keep both copies. Nothing is ever only deleted |


---

## Build 149 (9 October 2026) — SMB, the 3DS camera, Amiibo

CI run [37979559189](https://github.com/2c2bhdhw9z-cell/Continuum-/actions/runs/37979559189)
succeeded. Commit `d2a92a9`. Published:
[Continuum.ipa](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-149-d2a92a9/Continuum.ipa).

Builds 147 and 148 did not ship. Both died compiling `smb_min.c`. 149 includes `smb2.h` before
`libsmb2-raw.h`, which is where `smb2_file_id` is defined. The core cache from 147 is reused.

- **SMB file shares.** Back in. libsmb2 is compiled for iOS arm64 and linked into the app
  (`scripts/fetch-libsmb2.sh`, pin `fc710a3`). There is no framework to forget. Build 117 died
  because AMSMB2.framework was linked and not embedded. Listing shares, listing a folder and
  downloading a file go through `smb_min.c`. Passwords stay in the Keychain. Not tried against a
  real NAS from this build.
- **3DS camera.** Azahar's libretro frontend never asked for a camera, so the feed Continuum
  already had went nowhere. The patch registers a `libretro` camera and points all three 3DS
  cameras at it. Frames are the raw XRGB8888 feed, turned into the RGB565 or YUY2 buffer the game
  asked for. The Settings switch still has to be on. Not on a phone.
- **Amiibo.** A tap writes `continuum-amiibo.bin` and then `continuum-amiibo.stamp` into the save
  directory Azahar is given. Each frame, while a 3DS game is looking for a tag and none is already
  active, Azahar's own `LoadAmiibo` runs. A tap before the game asks is kept. Not on a phone.
- **Symbian / N-Gage.** Not in this build. No libretro core exists, and a wrapper around EKA2L1
  is not started. See Next up.
- **Feedback.** The address was already `idkplswrk@gmail.com`. The lines that said it was missing
  were wrong.

## Build 146 (9 October 2026) — the icon that is actually set, and settings that fold

Release `build-146-efd798e`, commit `efd798e`.
[Continuum.ipa](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-146-efd798e/Continuum.ipa).

- The home-screen icon grid marked Default even after another icon was set. On a sideload,
  `alternateIconName` can stay empty inside the success callback. The grid now stores the name it
  passed to `setAlternateIconName` and marks that. The selected cell has a red ring, a check, and
  a red name.
- Every Settings section opens and closes. Open all / Close all is on the Settings title. Closed
  sections are remembered.

## Build 145 (9 October 2026) — 55 home screen icons

Release `build-145-1a28e84`, commit `1a28e84`.
[Continuum.ipa](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-145-1a28e84/Continuum.ipa).

- Settings has a switcher for 55 alternate home screen icons, plus the primary icon.

## Build 144 (9 October 2026) — library scroll, re-import, fast-forward, the phone's name

Release `build-144-74050e5`, version 0.8.0 (144), 35 core files checked in the ipa.
Link: https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-144-74050e5/Continuum-144.ipa

The owner tried the four things that can be tried on one phone and said they look fine:

- **Done.** Home's bar stays readable once you scroll past the big cover. Home and All Games scroll fine.
- **Done.** Importing a game that is already in the library keeps it, and it still opens.
- **Done.** Fast-forward lets go when Control Centre takes the touch.
- **Done.** The (i) line names the phone.

Not shown, so not Done: rewind letting go the same way, covers not flashing, the phone name on the
feedback screen, one bad skin entry not wiping the list, and a random skin id not becoming a
console. A skin that does not name itself can still show up twice on two phones. That is on purpose.

## Build 142 (7 October 2026) — the owner's app icon, and everything below that was waiting on a build

**The app has an icon the owner supplied.** A chrome C around a gamepad with CONTINUUM beneath it,
on near-black.

**They do not like it, and said so plainly.** It is in the build because it was the last image
their free ChatGPT allowance would generate and they decided it would do for now. Read it as a
placeholder they are stuck with rather than a design anyone chose: a request to replace it is
expected. What is actually settled is the pipeline around it, which takes any square picture.

Source kept at `docs/app-icon-source.png`, the 13 sizes generated from it by
`scripts/make-app-icon.py --from`. Two things that had to be done to it, both reported by the
script as it ran rather than assumed:

- **Its transparency was flattened onto black.** The file arrived RGBA and iOS refuses an icon
  with an alpha channel — this is the usual reason a hand-made icon silently never appears.
- **The black border around its pre-rounded tile was trimmed**, measured at 44/40/44/51 px. This
  is the one that would have looked wrong rather than failed: the artwork is drawn as a rounded
  tile on black, which is what an app icon looks like, but iOS applies its own squircle mask with
  a LARGER corner radius than the drawn tile's. Left alone, the home screen would show the
  system's curve with the artwork's own narrower curve inside it and a black crescent between the
  two, at every corner. Trimmed, the tile runs edge to edge and iOS rounds it once.
  `trim_rounded_border` measures this by walking in along the middle row and column, so a
  full-bleed picture measures zero and is left untouched.

The trim leaves a 922 px master, so the 1024 `ios-marketing` slot is a mild upscale. That slot
exists for an App Store listing and this app is not on the App Store; every size the phone
actually uses is 180 px or smaller and downscales from 922, which is ample.

The getting-games-in work below rode along in this build.

---

## Built in 142 — the real scrolling cause, the cover flash, and the device name

**The first two scrolling attempts did nothing on any tab, and the owner was right to say so.**
Both were guesses. This one was measured.

`ArtworkStore` has **fifteen `@Published` properties**, and most are diagnostics that change on
every cover that resolves: `resolvedThisRun`, `missedThisRun`, `failedThisRun`, `line`, the
stored-file counts. `LibraryShell` observed that whole store while reading exactly **one** thing
from it, `generation` (four call sites, nothing else). So every counter bump invalidated the entire
library view — hero, every shelf, the grid, every visible card — and `sweepLibrary` resolves art
for all 79 games one after another in the background, so that is dozens of full rebuilds of the
library *while it is being scrolled*. Caching the shelf computation (build 141) could not help: the
rebuild was the cost, not the sort, and it only touched Home while the owner was on All Games.

Two comments in that file already warned about this exact hazard, next to `optionCache` and on
`syncAwaitingChoiceCount`. The counters were published anyway.

Fix: `ArtworkGeneration`, a one-property observable the shell watches instead. `LibraryShell` now
holds the store as a plain `let`. `generation` is unchanged for the detail sheet and Settings, which
want the counters and are not being scrolled.

**The cover flash, same root area.** The owner: *"if the game isn't shown then it does what you're
seeing... only like that for .1 seconds then fades back to the cover art."* Nothing was unloading.
The grid is a Lazy container, so a card scrolled back into view is built fresh with `cover` nil, and
resolving went through `cover(for:)` — which is `async`, so even a straight in-memory cache hit cost
a suspension, and the card drew its plate for that hop. `cachedCover(for:)` now answers from the
same `NSCache` with no awaiting, so a cover already in memory draws on the first frame, no plate and
no fade. The fade is keyed on the new value too, so it only animates a cover that genuinely had to
be fetched.

**The device name.** The report said `iPhone: iPhone18,2`, which is a part number rather than an
answer. It now reads `iPhone 17 Pro Max (iPhone18,2)`: the name when known, **the identifier
always**. Both, because the identifier is what `device_has_txm` matches on to decide JIT, so a
report without it cannot be diagnosed, and because a hand-written name table goes stale every
September — a phone missing from it reports exactly what it did before. Names verified against
theapplewiki's per-model pages rather than guessed.

---

## Built in 142 — the core cache was being deleted a second after it was restored

**The ~6 minute build never happened, and build 142 is the proof.** Its log reads
`Cache hit for: ios-cores-macOS-1c0a08df...` and `Cache restored successfully`, then rebuilt all
35 cores anyway and spent 35 minutes doing it. Zero cores were reused.

The cache was working perfectly. `native/ios/build-engine.sh` was throwing it away: line 39 was
`rm -rf "$OUT"`, and `$OUT` is `native/ios/build`, whose `lib` subdirectory is exactly where the
cache is restored to. Restore put 35 dylibs there; the next script deleted them.

Diagnosed by checking rather than assuming — the two candidate explanations were a key miss and a
failure of `ios_core_is_cached`, and the log ruled both out by reporting a hit while reusing
nothing, which left only something removing the files in between.

The clean slate stays, because a stale staticlib or a half-written dylib from an interrupted run
must not leak into a build. The core dylibs and `core-sources.txt` now step aside and come back
around it, and **only** under `CONTINUUM_CORE_CACHE=1`, so a build run by hand still starts from a
genuinely empty directory. Nothing else is preserved: the engine, the wrapper and the bindings are
rebuilt every time, because those are what a commit actually changes. Verified against a faked
build directory: the cores and manifest survive, `libemulator_bridge.a` and stale generated
bindings do not, and without the flag the directory comes out completely empty.

Build 142 was already past that point when this was found, so it paid the full 35 minutes. The
build after this should be the first genuinely fast one.

---

## Built in 142 — the app ASKS about a backup instead of waiting to be found

The owner's reply to being told to set a sync folder up: *"I've never had a chance to do it before,
so I'm not going to start now when this should have been one of the first things ever done in the
whole project."* They are right, and it is a product failure rather than their oversight.
Everything needed to survive deleting the app has been sitting in Settings for builds; they delete
the app before every single install; and nothing ever told them it was there. A feature nobody is
told about is a feature nobody has.

`BackupFolderPrompt` (CloudSync.swift, attached next to `CrashReportPrompt` on the root view) asks
once, in plain words, the first time there is actually something to lose — when the library stops
being empty, never on an empty first launch and never over a running game. "Not now" is remembered
and the Settings row is still there.

**Why it still takes one tap, which is the honest limit.** iOS deletes an app's entire sandbox with
the app and gives it no storage outside that survives. The one place that does is a folder the USER
grants through the system picker, and that grant cannot be pre-filled — it is the point of it.
iCloud's own container needs no picker and is not available here: re-signing an app on the phone
strips the iCloud entitlement, which is exactly why this sync was built around a chosen folder.
After the one tap it is automatic forever: `syncIfConfigured` already runs when the app opens and
when a game is left.

---

## Also in build 142 — deleting the app no longer loses everything

The owner deletes Continuum before installing each new build, every time, and said they hate it
but assumed it was unavoidable. It is not. Cloud sync already restored save states, battery saves,
cheats, settings, cover choices, manuals and Amiibo. The three things it did not are now fixed, and
every one of them turned out to rest on something fixable rather than on anything per-phone.

**Favourites survive a reinstall, and sync.** They were stored by absolute path, which contains
this install's container id, so after a reinstall they matched no game and were simply gone — and
that same path was the stated reason they could never sync, since another phone's paths would have
replaced its own list. Now keyed by **file name** (`continuum.favourites.v2`), the identity
`skinGameKey`, `import.systemChoices.v1` and `manuals.attached.v1` already use, and games live flat
in Documents so a name is unique. A new key rather than a rewrite, so an older build that would
read a name as a path cannot be handed one; v1 stays excluded forever and is read once to migrate
by taking each path's last component, which means favourites set *before* a reinstall come back too.

**Skins sync.** The exclusion was circular: the index could not sync because the bytes did not
sync, so an index arriving from elsewhere would name art the phone did not have. Syncing the bytes
removes the reason. `Skins/` covers all four shapes a skin id owns — `<id>.(pdf|png)`, the
`-landscape` variants, `sounds/<id>.caf` and `pieces/<id>/<file>` — against
`EngineHost.skinsDirectory()`. `pieces` is a level deeper than anything else that syncs, so it is
walked rather than listed flat, and its case checks the shape of the path rather than an extension
because skin authors use whatever they like. With the bytes travelling,
`continuum.skins.library.v1`, `continuum.controls.skinEdits.v1` and the
`continuum.controls.touchSkins.` prefix all come off the exclusion lists. Skins arrive in two
halves — files here, index in the settings plist — so the status line says they apply at the next
launch rather than reloading an index that has not been read yet.

**The games themselves, as a switch that is OFF by default.** Everything else that syncs is small;
a shelf of PlayStation discs is tens of gigabytes, and pushing that into somebody's iCloud without
asking would be a worse surprise than the problem it solves. With it on, `Games/` covers the top
level of Documents. The extension test is deliberately **wider** than `CoreCatalog.isLaunchable`:
a PlayStation `.bin` is a disc track and never a Library row, so `isLaunchable` refuses it, and a
`.cue` restored without its tracks is a game that cannot load. Firmware is excluded **by name**
through the new `CoreCatalog.isFirmwareName`, not by extension, because `.bin` is both a disc track
and the extension of half the BIOS files in existence. When games arrive the Library is rescanned
immediately, since a game is usable the moment it is on disk.

No Rust rule change was needed: `propagates_deletion` is false for anything outside `SaveStates/`
and `Artwork/covers/`, so a skin or a game missing on one side is copied back rather than deleted,
the same as a manual. The Rust test for that now covers all four skin shapes and a disc game with
its track (614 tests pass).

Owner tests: [TESTING.md](TESTING.md) M1 to M3.

---

## Build 141 (7 October 2026) — the Home screen: all your games, in the right order, scrolling better

Four things, all from the owner using build 138.

**"Recently added only goes to 18 even if I add 80 games at once."** It was
`Array(recent.prefix(18))` in `LibraryShell.swift`, an uncommented magic number, and the shelf
header read "18 titles" as though that were the whole library. The cap is gone. Nothing needed it:
the per-system shelves have always been uncapped, and `ShelfRow` renders inside a `LazyHStack`
precisely so a long shelf only builds the cards actually on screen.

**The order was wrong too, which is worse than the cap.** The shelf and the featured game were
sorted by the ROM file's own modification date. A copy into Documents can carry the SOURCE file's
date, so a game imported today could sort as though it were years old and never appear at all; and
when a batch of copies all land inside the same second, every entry ties and the sort silently
falls back to alphabetical — so "the 18 most recent" was really "the alphabetically first 18 of
whatever tied". The app now stamps arrival itself, one instant per file as it lands, under
`continuum.library.arrivedAt.v1`, **keyed by file name**. That is the scheme `skinGameKey` already
uses, and for the reason that matters here: a path holds a container id that does not survive the
app being deleted and reinstalled, and this owner deletes the app before every install. Being a
`continuum.` key it also rides along in cloud sync's settings half for free. Games with no stamp
(imported before this build, or dropped into Documents through the Files app, which never goes
through `importFiles`) fall back to the file date exactly as before.

**Scrolling, part one: stop re-sorting the library on every frame.** `shelves` was a computed
property read straight from the view body, so SwiftUI re-sorted the whole library and rebuilt the
per-system grouping on EVERY redraw — and a redraw happens whenever anything on the host publishes,
including each cover that finishes loading while you scroll. It is now a pure static function whose
result is cached in `@State` and rebuilt only on an import, a delete, or the per-system switch.

**Scrolling, part two: the cover cache was far too small, and the fix is NOT shrinking the art.**
The decoded-cover `NSCache` had a flat 64 MB ceiling. A decoded libretro box art is around 2 MB, so
that held about **thirty-two covers** — with eighty games, scrolling evicted constantly and every
card scrolled back into view paid a fresh disk read and JPEG decode. The ceiling is now a share of
the phone's actual memory (an eighth, clamped between 96 MB and 640 MB), which is ~500 MB on a 4 GB
iPhone and holds a couple of hundred covers, while an older 2 GB phone is pushed no harder than the
flat limit pushed it before.

Downsampling the covers would also have fixed that arithmetic, and was explicitly rejected: the
owner's instruction was that the art should look BETTER, not smaller. Nothing in this build reduces
a cover's resolution. Instead the covers are now drawn with `.interpolation(.high)` and
antialiasing, where they previously used SwiftUI's default (medium) resampling. Every cover is
scaled — a shelf card draws box art smaller than its 600x850, the hero draws it considerably
larger — so the default was visibly soft in both directions. Same source resolution, better filter,
a little more GPU time per frame.

Owner tests: [TESTING.md](TESTING.md) L1 to L3.

### Still to do for "I delete the app before every install"
Cloud sync already restores save states, battery saves, cheats, settings, cover choices, manuals
and Amiibo. Three things it still does not, now written down rather than discovered again:
**the games themselves** (never synced), **skins** (deliberately excluded), and **favourites**
(excluded because they are stored by absolute path, while the rest of the app keys games by file
name — the same flaw this build just fixed for arrival order). That is the next batch.

---

## Build 140 (7 October 2026) — two tester-facing lines that had gone wrong

Both found by reading the owner's build 138 Settings screenshot, not from code.

- **The JIT line now says what to do about it.** It read "Off. The way this copy was signed
  doesn't allow JIT. That's fine, everything still works, just slower on the heavy systems." The
  owner asked what that meant, which is fair: it names a cause, gives no action, and sounds
  permanent. It is not permanent — it is `classify(get_task_allow: false, debugged: false, _)`,
  decided entirely by how the .ipa was signed, and `get-task-allow.entitlements` ships beside the
  app on every release for exactly this. So the line now tells the reader that signing it again
  with that file gives them the option. No file path or web page is named, because testers get the
  app as a file in a chat group and never see a releases page.
- **"and N64 on older iPhones" was out of date the moment build 138 shipped.** The N64 was the
  last system that could not use JIT on an iPhone 13 or newer running iOS 26, and build 138 fixed
  that, so the qualifier was simply wrong. The line is now "Makes PSP, 3DS, PlayStation, N64 and
  Dreamcast faster."

Owner test: [TESTING.md](TESTING.md) J3. **J1 passed on build 138** (TESTING.md records the exact
sentence and that the StikDebug button was correctly absent).

Also worth recording from that screenshot: the library read **0 games**. If the owner's games were
in the app before updating to 138, that is a bug worth chasing; if they had not imported anything
into that install yet, it is nothing. Not yet known which.

---

## Build 139 (7 October 2026) — a transient GitHub error no longer costs a whole build

**Build 135 failed for a reason that had nothing to do with this project.** The .ipa was built,
verified and uploaded; then GitHub's own API answered **HTTP 500** to the release creation and the
whole 40 minute job went red with a perfectly good app inside it that the owner had no way to
download — an Actions artifact needs a logged-in GitHub account, and the entire point of that step
is the unauthenticated link.

`Publish the .ipa as a Release` now retries five times with growing gaps. `gh release create` is
not atomic — it can create the release and then fail while uploading an asset — so a retry that
meets an existing release treats that as success and re-uploads the assets with `--clobber`, which
is idempotent. If all five fail it says so and points at the artifact.

The icon check added in 135 is also proven by that run: its log reads
`app icon: none in the source tree, so none expected in the bundle`, which is the gated branch
behaving correctly now that there is no icon.

---

## Build 138 (7 October 2026) — the N64 gets JIT on current iPhones too

The last system that could not use JIT on an iPhone 13 or newer running iOS 26 now can, so
**every system with a JIT build gets it on every phone that can switch it on**. Not tested on a
phone: the owner's phone cannot use JIT at all, so this is code-complete and unproven, like the
rest of the JIT work.

**The reason this was listed as blocked was wrong, in two separate ways.** It was recorded as
"parallel-n64 writes its jump trampolines through the same pointer it runs them from, so it cannot
use two-address code memory". Checking before writing any code:

1. **That trampoline allocator is never called.** `trampoline_arm64.c` is compiled (it is in
   `Makefile.common`), but at the pinned commit the only reference to any of its functions
   anywhere in the core is `apple_jit_protect.c` calling `trampoline_commit()`. Nothing calls
   `trampoline_init`, so `DataBase` and `CurrentData` stay NULL, nothing is ever allocated, and
   `trampoline_commit()` is a no-op. `assem_arm64.c`'s three mentions of "trampoline" are all in
   comments. It is dead code.
2. **The recompiler already supports two addresses.** `new_dynarec.c` defines
   `DOUBLE_CACHE_ADDR` — "Put the dynarec cache at random address with RW address != RX address" —
   and the Switch port uses it, because the Switch has the same restriction iOS 26 does. Every
   translation that needs is already written: about twenty-five in `new_dynarec.c` (the
   `((intptr_t)x - (intptr_t)base_addr) + (intptr_t)base_addr_rx` pairs around the jump and hash
   tables) and thirteen in `assem_arm64.c`, where `out_rx` is derived from `out` for every
   PC-relative branch and for the jump table. All four `cache_flush` call sites already pass run
   addresses.

So no new machinery was needed, only the host's pair wired into the two pointers the recompiler
already has. `scripts/patches/parallel_n64-ios-jit-region.patch`:

- Asks the host for a 32 MB region (`TARGET_SIZE_2` is 25) via
  `dlsym(RTLD_DEFAULT, "continuum_jit_region")`, the same lookup the other four core patches use.
- Sets `base_addr` to the **write** address (`out` is based on it) and `base_addr_rx` to the
  **run** address.
- Leaves `jit_region` NULL in that mode, which is what makes `jit_write_enable` and
  `jit_write_disable` no-ops: neither view's permissions ever change, so there is nothing to flip.
- Falls back to the existing single mapping with `mprotect` flipping when no host region is
  available, so an iPhone 12 or older behaves exactly as it did in build 133.
- Gives the region back by its run address in `new_dynarec_cleanup` instead of unmapping memory
  the host owns and hands to whichever core asks next.

`jit::core_may_use_jit` now allows `parallel_n64` inside the region, and its test was rewritten to
assert that every patched core is allowed and that melonDS (which has no iOS JIT build at all) is
not.

**How it was checked, since the sandbox cannot build for iPhone:** both patches were replayed onto
a pristine checkout of the pinned commit in order, and both applied cleanly; brace balance across
`new_dynarec.c` is unchanged; and the new C was lifted into a standalone harness and compiled with
`clang -Wall -Wextra`, with no warnings, where it correctly took the fallback path and left
`base_addr == base_addr_rx` when no host was present. `RTLD_DEFAULT` needs `_GNU_SOURCE` on Linux
but is unconditional in Apple's `dlfcn.h`, and the four already-shipping core patches use the same
call. 614 Rust tests and clippy pass.

Note for whoever edits this next: `ios_apply_core_patches` applies
`parallel_n64-ios-jit.patch` **before** `parallel_n64-ios-jit-region.patch`, and the second one's
context is the file as the first leaves it. That order is load-bearing.

---

## Build 136 (7 October 2026) — the build takes ~6 minutes instead of ~40

Nothing in the app changed. This is about how long a fix takes to reach the phone, which was
the worst thing about working on it: every build, including one that only changed a line of
documentation, spent **35 of its 40 minutes rebuilding 35 emulator cores from source**, and
anything that went wrong late in the job meant paying all 35 again for the retry.

Measured from build 134's own timings before changing anything: 35.1 min of 39.5 in one step,
88.9% of the job, spread across the per-core builds (snes9x 1.2 min, stella2023 1.3 min,
azahar's submodules 2.1 min, and so on). Everything else together is under 5 minutes.

Every core is pinned to an exact upstream commit, so its dylib is a pure function of that
commit, `build-core.sh` and the patches. So they are kept between runs:

- `scripts/build-core.sh` gains `ios_core_is_cached`. It reuses a staged dylib only when
  `ios_record_source_version`'s provenance manifest has a line naming that core's repository,
  its **pinned sha**, and the word `pinned` — which that function writes only when the
  checkout's HEAD really was the pin. A dylib left from a different commit cannot satisfy it.
  Off unless `CONTINUUM_CORE_CACHE=1`, which only CI sets, so a build run by hand is unchanged.
  Verified against a faked staging directory: reuses on an exact pinned match, and rebuilds
  with the environment variable off, with no manifest, when the manifest names a different
  commit, and when it says `unpinned`.
- **It also refuses to reuse anything under `CONTINUUM_UNPINNED=1`.** That switch exists to
  build every core at its default branch's HEAD instead of its pin, which is how a newer
  upstream core gets tried on purpose. Without that guard the request would have been
  silently ignored — the restored manifest still says `pinned`, so the check would pass, the
  build would be skipped, and the run would ship the OLD dylib while reporting success. This
  was the one way the cache could genuinely have hidden a core update, and the owner spotted
  it before it ever ran.

### Why caching the cores cannot miss a core update

Worth stating plainly, because "cached build ships the old dependency" is the normal way this
goes wrong and it is the reason it had not been done before:

- **Nothing here tracks upstream.** All 16 from-source cores are pinned to a full 40-character
  commit id in `ios_core_config` (the one empty `IOS_PIN=""` is that function's initialiser,
  not a core). The pins are deliberate: see the note above `ios_clone` — "an upstream commit
  landing overnight must not be able to change or break a build nobody touched". A core does
  not change because upstream moved; it changes when someone edits a pin in this repository.
- **Moving a pin invalidates the cache twice over.** The cache key hashes `build-core.sh`, so
  editing a pin misses the cache entirely; and independently, the per-core manifest check
  cannot match a dylib recorded against the old commit. Either alone would be enough.
- **Asking for newer code on purpose bypasses the cache**, per the `CONTINUUM_UNPINNED` guard
  above.
- **The prebuilt cores are never cached in any meaningful sense.** `fetch_pinned` in
  `scripts/fetch-buildbot-cores.sh` does `rm -f "$dest"` and re-downloads and re-verifies the
  sha256 of every file on every single build. A restored copy is overwritten by a freshly
  checksummed one, so those cannot go stale either.
- `.github/workflows/ios.yml` caches `native/ios/build/lib/*_libretro_ios.dylib` plus the
  manifest, keyed on `hashFiles('scripts/build-core.sh', 'scripts/patches/**',
  'scripts/fetch-buildbot-cores.sh')`. The patches are in the key because a patch changes a
  core's output without changing any pin; editing one throws the cache away and everything
  rebuilds, which is correct.
- **The cache is saved immediately after the cores exist**, using `actions/cache/restore` and
  `actions/cache/save` as separate steps rather than one `actions/cache`. The combined step
  saves in post-job cleanup, which only runs on whole-job success — precisely the wrong
  moment: a job that built all 35 cores and then failed at `xcodebuild` or the release step
  would save nothing and the retry would pay the full 35 minutes.
- The save is deliberately **not** `if: always()`, which is how it was first written and was
  wrong. Sitting before `xcodebuild` already means a later failure cannot reach it, so
  `always()` adds nothing there — all it would add is saving when the core build ITSELF failed
  or was cancelled, which is exactly when the set is INCOMPLETE. A partial set is worse than
  none: it stores under this key, every later run sees a cache hit, skips saving, rebuilds the
  missing cores every time, and nothing ever says why the build is still slow.

Build 136 itself is still slow — it is the one that fills the cache, and its key changed
because `build-core.sh` did. Builds after it are the fast ones.

---

## Build 135 (7 October 2026)

**The drawn app icon is out again, and the app ships no icon.** Nothing else changed from 134, so
every build 133 test still applies.

Build 134 added a drawn icon and should not have. The owner had asked to see artwork before it went
into the app; it was committed and built before they saw it, and when they did see it — as one of
four options on a sheet — they rejected all four. Shipping art they had rejected was the wrong call,
so 135 removes it.

What was taken out: `native/ios/Assets.xcassets`, the `- path: Assets.xcassets` line and
`ASSETCATALOG_COMPILER_APPICON_NAME` in `native/ios/project.yml`, and the two preview sheets in
`docs/`. The app installs as a blank white square again, which is the state until the owner supplies
or approves a picture.

What was kept, because it is the mechanism rather than the art:

- `scripts/make-app-icon.py` **has no default mode any more**. `--from <picture>` fits any supplied
  picture to all 13 sizes; `--drawn` uses the rejected triangle if it is ever asked for; running it
  with no arguments prints usage and exits 1. That is deliberate: putting an icon in the app can no
  longer happen as a side effect of running a script.
- `--from` reads the real format from the file's **bytes** rather than its name, flattens any
  transparency onto black (iOS refuses an icon with an alpha channel, and that is the usual reason a
  hand-made icon silently never appears), and centre-crops to square. So "send a picture and it gets
  fitted" is one command.
- The `.ipa` verify step in `.github/workflows/ios.yml` still checks the icon, but **gated on
  `native/ios/Assets.xcassets/AppIcon.appiconset` existing in the source tree**. It cannot fail a
  build over a missing icon nobody asked for, and it cannot be quietly satisfied by one that failed
  to arrive. Both halves are checked when there is art: `Assets.car` in the bundle, and the
  `CFBundleIcons` keys in the built `Info.plist`. Confirmed working on build 134, which passed both.

The drawing code and its comments are left in place as a record of what was tried, including three
things measured rather than eyeballed: ramping the shape from the accent red to the metadata teal
passes through grey because those two are near-complements, a highlight on a red surface has to be a
brighter red rather than the complementary colour, and stroking a closed outline with Pillow's
`joint="curve"` leaves flat caps where the polyline starts, which bit a square notch out of the
triangle's top-left corner.

Also recorded for whoever tries this next: the keyless image service (Pollinations) now answers only
on its oldest small model, ignores the requested size and returns 768x768, and **burns a
`pollinations.ai` watermark into the corner even with `nologo=true`**. Watermarked art cannot ship,
so that route is closed regardless of how the output looks.

---

## Build 134 (7 October 2026)

A drawn app icon, removed again in 135 — see above. Do not install this one for the icon.

---

## Build 133 (7 October 2026)

Checked in the .ipa: the app exports the two symbols the cores look up, and PPSSPP, Azahar, PCSX
ReARMed and flycast's JIT build all have the lookup compiled in. Nobody has run it on a phone with
JIT on yet.

**JIT now works on current iPhones.** Build 131 only covered phones where attaching a debugger is
the whole job, which on iOS 26 means an iPhone 12 or older: almost nobody. This build does the part
that was missing.

- An iPhone 13 or newer on iOS 26 has TXM, which means the app may only run generated code from a
  single region that a debugger blessed, and that region stays read-and-execute for good. So the
  engine reserves one 512 MB region, has the JIT app bless it through StikJIT's universal protocol
  (`JIT26PrepareRegion`, then `JIT26Detach`, in `crates/emulator-bridge/src/jit26.c`), and maps a
  second writable address for the same pages with `vm_remap`. Cores take slices of it through an
  exported `continuum_jit_region`, getting one address to run code from and one to write through.
- The breakpoint that protocol uses is only ever executed with the right script attached: the app
  asks StikDebug for `universal.js` itself, waits until the kernel says a debugger is attached, and
  only then runs it. With nothing listening that instruction would close the app, so there is also
  a plainly labelled button for someone who attached by hand.
- Patched to use it: **PPSSPP** (PSP), **Azahar** (3DS, through oaknut and dynarmic), **PCSX
  ReARMed** (PlayStation) and **flycast** (Dreamcast). Each already had a separate-write-address
  mode for the Nintendo Switch; this points it at the host's region. The host hands out the same
  pair of addresses on older iPhones too, so there is one code path everywhere.
- **The N64 is the exception** and stays on its interpreter on those phones; see "Next up".
- The 3DS recompiler's code cache is 32 MB per emulated process on a phone instead of 128 MB, so
  several fit in the region.
- Settings says what is happening in plain words, and finishes the setup by itself after the
  StikDebug trip. Nothing changes on a phone without JIT, including the owner's.

## Build 131 (7 October 2026)

(Build 130 stopped on a Swift start-up order mistake and an N64 JIT link error; both fixed.)
JIT for everyone who can switch it on (owner, 7 October). Nothing changes on a phone without JIT,
which includes the owner's: every core loads and runs exactly as in build 129.

- The engine reads the app's own signing flags (`crates/emulator-bridge/src/jit.rs`): JIT is "on"
  when a debugger has attached (`CS_DEBUGGED`) and the phone is not an iOS 26 TXM phone. Read
  live, so StikDebug can switch it on while the app is open; nothing is ever executed to find out.
- PSP (PPSSPP) and 3DS (Azahar) use their recompilers from their regular builds: the host now
  answers `RETRO_ENVIRONMENT_GET_JIT_CAPABLE`, PPSSPP's CPU option defaults to "JIT" when the
  answer is yes, and a Continuum patch makes Azahar ask instead of hard-coding the interpreter.
- PlayStation (PCSX ReARMed), N64 (parallel-n64) and Dreamcast (flycast) get second builds with
  their recompilers on, `<core>_jit_libretro_ios.dylib`, loaded instead of the regular build only
  when JIT is on. Patches: PCSX ReARMed maps its code cache at run time and uses 16K pages; the
  N64 recompiler switches pages with `mprotect` instead of the macOS-only call. Optional builds:
  if one fails to compile, that core simply has no JIT in that build.
- Settings, Technical details: a JIT line in plain words, "Use JIT when it's available" (on by
  default, off sends every core back to its regular build from the next game), and "Turn on JIT
  with StikDebug" where attaching is all it takes. The (i) panel and the feedback details carry a
  `JIT:` line with the state, the phone and which JIT builds are in this copy.

## Build 129 (6 October 2026)

Cleaned up for a public beta: strangers will download and test it. (Build 128 was cancelled
part-way, because the owner asked for every tester-facing line to sound human first.)

- Every line a tester reads (feedback form, crash prompt, Settings notes, empty Library, cheat
  and save messages) rewritten in a casual, first-person voice.

- The player shows no technical text over the game. Status lines show for five seconds (ten for a
  problem) and fade; fps, frames and the full technical block are behind the (i) button.
- Settings: Feedback first; Technical details (the old Diagnostics) and About with the version
  last. Developer notes removed (JIT test button, graphics "step 4", PSP routing notes); the
  PlayStation, PSP and BIOS notes rewritten for players.
- The Library strip says how many games, and the last status line only when it is a problem.
- Feedback goes by email to idkplswrk@gmail.com (the owner's address). Without Mail set up, the
  share menu text starts with that address.
- README and the Release notes are written for testers, and name no particular installer.

## Build 127 (6 October 2026)

The owner tested build 126: A1, A2, A4 pass; the rest is below. TESTING.md A1 to A4.

- 3DS: changing System Model (New 3DS to Original) mid-game crashed the app, with no restart
  button. Azahar says "Restart required." only in an option's description, and the engine read
  only the label, so nothing waited for a restart; and Azahar re-reads every option on an update,
  so it switched the emulated console under the running game. The engine now reads the
  description too, and holds a restart-required option at the value the game started with until
  the next start (`cores/options.rs`, `answer`). Each option also reports the value really in
  effect, and save states are stamped and checked with that.
- Cheats: a switched-off cheat kept running on the GBA and Game Boy. mGBA ignores the on/off flag
  and adds every code it is given; the engine now sends only the cheats that are on, the way
  RetroArch does. The owner's walk-through-walls code was the FireRed version 1.1 CodeBreaker
  code, which freezes other versions; the cheat screen now names the exact game and version from
  the GBA or Game Boy header, and says a code for another version can freeze the game.
- Feedback, rebuilt after the owner called it basic: a rating and what is wrong for a game, drawing
  on the picture, the activity log attached, the tester's name, a sent count, and a crash report
  offered after the app closes by itself (engine: `feedback.rs`). A save that was loading when the
  app closed is skipped once, so a bad save cannot crash every start of a game.

## Build 126 (5 October 2026)

The owner tested build 125 (results in HANDOFF.md). The fixes and the two asks. TESTING.md A1 to A6.

- 3DS save states: every one was refused as "too short" (14.6 MB saved, the core asking for 19.2,
  then 17.5). The app compared a state with the size the core reports right now, and Azahar's
  figure moves as the game runs (so do PPSSPP's and Flycast's). No length is compared any more;
  the engine pads a short state instead, so a core can never read past the end, and it now records
  the core's own "my size can change" flag (`SET_SERIALIZATION_QUIRKS`, under both numbers it has
  had, 44 and 87). Saving on the 3DS also does one full serialisation instead of two.
- An imported save had no picture. An export now carries the slot's picture (format version 2,
  only when there is a picture), and a slot with none gets one the first time it is loaded.
- With a skin that puts the game in its own screen holes, the small text was never shown and the
  (i) button did nothing. Each new status line now shows for five seconds and fades, and (i) opens
  the full block over the game.
- Cheats: a typed code (GameShark, Game Genie, Action Replay, CodeBreaker, CWCheat and so on) can
  now be added from the cheat screen inside a game, not only from the game's card. Both places say
  which kinds of code that system's emulator reads, or that it ignores typed codes (3DS, Dreamcast,
  Atari 2600, arcade and others), from a table in the engine (`cheats::formats`) read off each
  core's own source.
- Feedback (the owner's ask), in two places: Settings (the first card) and a game's ⋯ menu. A short
  form, plus the app's details and, from a game, a picture of it. It opens Apple's Mail screen
  addressed to the owner once there is an address (`FeedbackDestination.email`), and the share
  menu until then. Nothing is sent without the player tapping send.

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
  settings change, fast forward, the CRT filter, the cover from the game and Save to the next free
  slot.

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
  restart-required core settings are refused instead of crashing Azahar (confirmed 5 October on
  build 122).
- **121**: switch to hide Apple's performance overlay; docs brought up to date.

## Built 3 October 2026

Everything below is on master and in the current install, and mostly not on a phone yet. Confirmed
since (build 122): 2x speed, the CRT filter, a 3DS core-settings change and importing several
skins. Confirmed on build 125: Jaguar and Pokemon Mini each ran a game, saved and loaded; paste,
Open in and zip imports; cover lookups off. TESTING.md has an easy numbered list for trying the rest.

- **19 new systems:** WonderSwan, Neo Geo Pocket, PC Engine CD, SuperGrafx, Amiga, C64, DOS, DOOM, Jaguar, Lynx, Atari 7800, Atari 5200, Arcade, Pokemon Mini, Virtual Boy, Saturn, Sega CD, 32X, and Dreamcast (optional, never compiled before build 116).
- **Getting games in:** Wi-Fi transfer, paste, drag and drop, Open in, WebDAV, SMB (libsmb2 linked
  into the app; see build 149 above), zip and 7z files, automatic system detection, and
  save files in other emulators' formats.
- **Manic skins:** .manicskin files, a skin library, a skin per game, switching mid-game, press animations, switch buttons, button sounds, and all 48 function buttons.
- **Core settings for every core:** filters, palettes, 2x/3x/4x and slow motion, disc swap, rotation, and separate TV settings.
- **Controls:** a keyboard, tilt and shake, controller types, remapping profiles, DS lid and blow, and the 3DS HOME button.
- **Gameplay manuals:** PDF manuals attached to a game.
- **Also in the current install:** Flash, J2ME, memory maps (GBA achievements and RAM search on GBA), and better disc hashing for achievements.

### Not done

- SMB (NAS shares) is back in build 149: libsmb2 is linked into the app, not shipped as a
  framework. Build 117 closed on launch because AMSMB2.framework was linked and not packed. Not
  tried on a phone yet. WebDAV still works.
- GameCube and Wii: they cannot be playable without JIT (see docs/HARD_SYSTEMS.md).
- Symbian / N-Gage: Built, untested from the build after 150 (Continuum Symbian, see above).
- Direct Google Drive, Dropbox and OneDrive logins: they need developer app ids only the owner can
  register. They do work through the Files picker.
- The iPhone camera and an Amiibo tap are wired into Azahar in build 149. Not on a phone yet.
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
| 4. `SET_HW_RENDER` for Vulkan, against Beetle PSX HW | **Partial**. Frontend contract + live MoltenVK install path on `master`: after Metal attach, `prepare_vulkan_hw` creates a shared MoltenVK `VkInstance`/`VkDevice`/`VkQueue` (same extension filter as step 3); `SET_HW_RENDER` accept (or prepare if accept already happened) calls `install_vulkan_handles` and the core's `context_reset`. `set_image` records `VkImage` from `create_info`; tick tries `VK_EXT_metal_objects` → `adopt_frame_texture`. Host tests cover callback layout, interface field order, accept/refuse, `set_image`, and install→`context_reset`. **Beetle PSX HW is in `IOS_CORES` / `ios-all`** and in the IPA; Settings → PlayStation core → Beetle PSX HW selects it (PCSX ReARMed remains default). Launch now refuses without a user-supplied BIOS (no HLE, no fake BIOS) and the BIOS line names Beetle honestly. The game page "Runs on" line and the Library detail follow that same choice (since build 100; not checked on a phone). Build 98 device, with `scph1001.bin`: Crash booted on Beetle at ~60 fps, but on Beetle's **software** renderer. At the time the engine refused every core setting, which left Beetle's hardware renderer off, so the Vulkan path was never asked for. The engine now keeps Beetle on software by default (`host_rules` in `crates/emulator-bridge/src/cores/options.rs`); the hardware (Vulkan) renderer is a choice in Core settings, and its “Beetle HW: first Vulkan frame…” line has **not been seen on a phone**. Do not stamp step 4 Done. Brett said that boot is enough to move on to 3DS; do not reopen it in chat. iOS core builds remain Mac/CI-only |
| 5. Recompiler measurement | **Not built**, and blocked: no JIT on the owner's install |
| 6. **paraLLEl-N64**, the N64 | **Partial**. The software core is Done on build 97 (Smash character select, ~60 fps, no JIT). The Vulkan RDP and the recompiler are **not built**. Do not read that as “N64 is not in the app.” |
| 7. ANGLE alongside MoltenVK | **Partial**. OpenGL and OpenGL ES `SET_HW_RENDER` are accepted. They used to be refused; that was an app choice, not the phone. Preferred answer stays Vulkan, so Azahar and Beetle still take MoltenVK. On iOS the accept path creates an EAGL OpenGL ES context and an FBO, `get_current_framebuffer` returns that FBO, and `get_proc_address` is `dlsym` into OpenGLES. A hardware frame is read with `glReadPixels` into top-left RGBA8 and uploaded by the compositor that already shows software frames. Host tests prove the accept, the callbacks, that Direct3D is still refused, that a Vulkan request still wins, and that a bottom-left GL buffer becomes the exact RGBA the upload passes through. **Not device-proven.** No phone has shown this frame. **ANGLE is not in this build**: no Metal client-buffer, no zero-copy `MTLTexture` from GL. The copy is the gap. Desktop `OPENGL` / `OPENGL_CORE` are accepted so the core is not refused, but the context is OpenGL ES, because iOS has no desktop GL. A core that needs a desktop-only entry point will not draw. If ES3 cannot be created, ES2 is tried, and that ES2 context has depth but not stencil. Do not stamp step 7 Done |
| 8. **Azahar** (Citra fork), the 3DS | **Partial**. In `IOS_CORES` and the IPA as `azahar_libretro_ios.dylib`. **Device:** Mario Kart runs. Apple builds have OpenGL off and Vulkan on; this core presents with `set_image`. CPU JIT and shader JIT are compiled out by `-DIOS`. No dynarec, no executable memory. The hitch this IPA changes is in Azahar's Vulkan rasterizer: `async_shader_compilation` defaults off, so every new pipeline sets `wait_built` and `BindPipeline` calls `WaitDone` on the emulation thread. The Continuum patch skips that accelerated draw until the pipeline worker finishes. The software-vertex fallback still waits, because that caller ignores a failed bind and would record a draw with no pipeline. That is the smallest wait the source proves. It has been in every install since 2 October, and Mario Kart 7 has run with it on build 122, and on build 150 Brett confirmed the stutter is still there. Why it was not enough: `TryBuild` still asks the driver for the pipeline first with `FAIL_ON_PIPELINE_COMPILE_REQUIRED`. MoltenVK v1.4.2 honours that flag only when the SPIR-V to Metal library is missing; when the library is cached it builds the `MTLRenderPipelineState` synchronously on the emulation thread. The second patch (now also in `scripts/patches/azahar-do-not-wait-on-pipeline-compile.patch`) skips that probe when the caller will not wait, and the software-vertex fallback now skips a draw whose pipeline is still compiling. Built, not tried on a phone; do not call it fixed. A retail title may need system archives the app does not ship, and `SET_MESSAGE` is ignored |
| 9. **PPSSPP**, the PSP | **Partial**. In `IOS_CORES` and the IPA as `ppsspp_libretro_ios.dylib`. The iOS job compiles it for arm64. CPU is the IR interpreter (`IRJit` with compile-to-native false). No JIT, no dynarec, no executable memory. Vulkan `set_image`. No BIOS. A PSP game can be `.cso`, `.iso`, `.chd`, a PSP `EBOOT.PBP`, or `.prx`. `.cso` and `.prx` are always PSP. An `.iso`, `.chd` or `.pbp` is looked inside: a PSP disc or EBOOT opens on the PSP, a PlayStation one as PlayStation, and if the app cannot tell it asks "Which system?" once and remembers. `.elf` is not accepted. **Not tried on a phone.** The App Store PPSSPP build is also interpreter-only; lighter games are reported smooth there and heavy ones hitch. That is not this phone. Do not call it Done until a game runs on Brett's phone |
| 10. **Switch**, stage 1: the wrapper against a stub engine | **Partial**. `native/switch-wrapper/` already has the `retro_*` skeleton, the frame gate, a Vulkan stub renderer and a test harness. No Rust engine behind it |
| 11. Switch, stage 2: a real engine behind `ISwitchEngine`, homebrew booting | **Not built** |
| 12. Switch, stage 3: capability shim, shader cache, retail content | **Not built** |

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

**MoltenVK in the bundle** is about 8 MB of an app that is now about 85 MB. Every Vulkan core here
needs it (Azahar, PPSSPP, Beetle PSX HW), and a future paraLLEl-RDP would too.

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
  makes it a root before LTO runs. It has built cleanly in every build since, through 124, and the
  mechanism is deterministic rather than lucky.
- **The staged-dylib check now covers all 20 entry points**, not just `retro_run`. One symbol only
  ever caught a core that resolved nothing; a core that loses one entry point is both likelier and
  much quieter. That is not hypothetical: save states were broken for this project's whole life
  because `retro_serialize` was never resolved.
- **`swiftc` on Linux cannot type-check.** It is a syntax pass, so a Swift type error, an actor
  isolation mistake or a wrong argument label is only caught by CI. Several builds have been spent
  on exactly that, and it is a property of the toolchain rather than carelessness.
- **The core sources are pinned since build 122** (build 124 still uses build 121's versions). Every emulator built from source is checked out at the
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
- **PSP (PPSSPP)** is step 9 and is **in this IPA as Partial**, not Done. The CPU is the IR interpreter, no JIT and no dynarec. It has not been tried on a phone. App Store PPSSPP is also interpreter-only; lighter games are reported smooth there and heavy games hitch. That report is not a Continuum result. Nothing to do with Switch is deferred; it is steps 10 to 12. 3DS is in the app as Partial (a game runs; whether the stutter after transitions is gone is not known yet), not deferred.
