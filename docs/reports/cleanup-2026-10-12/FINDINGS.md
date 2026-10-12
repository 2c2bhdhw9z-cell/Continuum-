# Combined findings: cleanup 2026-10-12 (range `8c89e2c..HEAD`)

Sources: `review-ios.md` (24 findings), `review-rust.md` (19), `review-cboy.md` (12). All 55 are kept below with their original tag, severity, reference, evidence and fix. IDs have an area prefix: `IOS-`, `RUST-`, `CBOY-`. All three area verdicts were NEEDS_CHANGES.

**Overall verdict: CHANGES_REQUESTED**

Fix phase scope: fix CONFIRMED HIGH and CONFIRMED MEDIUM. Everything in the third bucket goes into the summary as "found, not fixed".

## Index

### CONFIRMED HIGH (fix): 5
- IOS-01: The backup prompt promises a kept folder but only makes one zip, once
- IOS-02: A refused resume lets the next exit overwrite the auto-save
- IOS-03: Tester feedback still goes to the retired address
- RUST-01: Symbian firmware folder can't be reached
- CBOY-01: Cartridge clock never saved (fix CBOY-03 and CBOY-09 in the same push, see that finding)

### CONFIRMED MEDIUM (fix): 15
- IOS-04: The feedback and crash-report forms open fully folded on a fresh install
- IOS-05: Back up to Files copies deleted things back into the app
- IOS-06: Restore always lets the backup's copy win
- IOS-07: Restore reports "restore finished" after a failed merge, and accepts any zip
- IOS-08: Back up says "saved" even when files failed to copy into it
- IOS-09: A damaged .zip crashes the app during Restore
- IOS-10: The default NES core is an optional download with no fallback
- RUST-02: Symbian's refusal is swallowed
- RUST-03: An old Amiibo is tapped again after a relaunch (same bug as IOS-12, see the overlap notes)
- RUST-04: What a game writes to an Amiibo is lost (mixed tag: the mechanism is CONFIRMED, the impact is SUSPECTED)
- RUST-07: cBoy is always rebuilt, yet it is in the shared cache key
- CBOY-02: Phone motion never reaches cBoy
- CBOY-03: HuC3 clock scrambled by the real-time catch-up (latent MEDIUM; goes live with the CBOY-01 fix)
- CBOY-04: Boot ROM / BIOS / custom palette can't work on a phone, and the text is false

### Everything else: SUSPECTED, or LOW (found, not fixed): 35
SUSPECTED MEDIUM:
- RUST-05: Symbian ignores pause
- RUST-06: Symbian reinstalls the game on every launch, on the main thread
- CBOY-05: Game Boy ROM run as an Advance game

CONFIRMED LOW:
- IOS-11, IOS-12, IOS-13, IOS-14, IOS-15, IOS-16, IOS-21, IOS-22, IOS-23, IOS-24
- RUST-08, RUST-09, RUST-10, RUST-11, RUST-14, RUST-15, RUST-16, RUST-17, RUST-18, RUST-19
- CBOY-06, CBOY-07, CBOY-08, CBOY-09 (latent; CBOY-01 asks for it in the same push), CBOY-10, CBOY-12

Mixed-tag LOW:
- IOS-17 (CONFIRMED on Java / SUSPECTED on iOS Files), IOS-20 (CONFIRMED text and port / SUSPECTED pool), CBOY-11 (CONFIRMED claim / SUSPECTED offset)

SUSPECTED LOW:
- IOS-18, IOS-19, RUST-12, RUST-13

### Overlaps between areas (both entries kept, neither re-graded)
- **IOS-12 (LOW) and RUST-03 (MEDIUM)** describe the same Amiibo re-tap bug, seen from the Swift side and from the patch side. RUST-03 is in the fix list, so fixing it also fixes IOS-12. IOS-12 adds one more point: success is judged by matching the text "tapped".
- **RUST-08 (LOW) and CBOY-07 (LOW)** describe the same problem: `Cart::new(...).ok()?` at `libretro.rs:587` throws away the refusal reason.
- **IOS-11 (LOW) and CBOY-06 (LOW)** both cover the cBoy picture shape (squashed by about 3%) from different causes: IOS-11 is `auto_sgb`, CBOY-06 is the border option applying live.
- **RUST-02 / RUST-08 / CBOY-07** all involve the `cBoy: `-only refusal channel at `native_core.rs:1310-1318`. RUST-02's fix (one refusal channel for the cores this project owns) is the shared fix.
- **RUST-01 and CBOY-04** share a root cause: the core system folder is Application Support, which testers can't see, and `installBiosFromDocuments` only copies `installableFirmwareNames` (`ContinuumApp.swift:3380`, `:3673-3683`).
- **RUST-11 and CBOY-01** both involve the clock-footer trim in `bridge.rs:1794-1802`. CBOY-01's alternative fix (keep the 48-byte footer in SAVE_RAM) would change RUST-11's fix.
- **IOS-03** also lists the retired address outside `native/ios`: README.md:52, STATUS.md:208/322/389/944, TESTING.md:282, HANDOFF.md:63, docs/MANIC_PARITY.md:178.

Not repeated here, already in REPORT.md §2 (noted by the Rust review): the clippy error at `native_core.rs:382-386`, and CI never running clippy.

---

## iPhone app (`native/ios`, paths under `native/ios/` unless stated)

Context: Build 180 compiled exactly this `native/ios` tree (`git diff dd89824 HEAD -- native/ios` is empty), so compile errors and missing `project.yml` sources are ruled out. The problems are in behaviour.

### IOS-01 · The backup prompt promises a kept folder but makes one zip, once
HIGH · CONFIRMED · `CloudSync.swift:1830, 1839-1840, 1856-1865, 1876-1886, 1818`
- Evidence: the alert says "If you pick a folder, Continuum keeps a copy … there … After installing again, choose the same folder in Settings and it all comes back". `choose` runs `sync.backUpToFiles()` once (a single exported .zip) and sets `askedKey`. `offerIfItIsTime` needs `!sync.isConfigured && !askedKey`, so the prompt never returns and no later backup happens. After a reinstall, "choose the same folder" finds no `Continuum Sync` folder, only a .zip. The doc comment still says "after that it is automatic forever". The buttons are "Choose a folder, with games / without games" (1839-1840); this behaviour has been in place since c67a111.
- Fix: rewrite the alert in plain words to match what it does, for example "Save a backup file now? After reinstalling, tap Restore from a backup and pick it." with buttons "Back up with games" / "Back up without games". Because nothing is automatic, offer it again when the last backup is older than about a week and saves have changed since. Fix the doc comment.

### IOS-02 · A refused resume lets the next exit overwrite the auto-save
HIGH · CONFIRMED · `SaveStates.swift:1401-1405, 1134, 1317`; `ContinuumApp.swift:787-791, 1016, 1318`
- Evidence: on `coreMismatch`, `versionMismatch` or `settingsChanged` (SaveStates.swift:1025-1040), `resumeIfPossible` returns without setting `autoSaveHeldFor`. `writeAutoSave` only skips when it is set, which happens only in the crash-guard branch (1317) and the PPSSPP wait (ContinuumApp.swift:4648), so leaving or backgrounding the game replaces the auto slot in place. NES now routes to `mesen2` (`routeTable["nes"]`, `defaultCoreBySystem[.nes]`), so every FCEUmm auto-save from build 152 or earlier is erased on the first exit. The per-game picker (which beats the Settings choice, 1318) and the cBoy choice for gb/gbc/gba do the same to any game whose core is switched. a4bb9da's message ("never overwritten by a session that has not loaded it") does not hold here. The Settings note (SettingsScreen.swift:620-621) says nothing about the auto-save being replaced.
- Fix: on any refusal, move the old auto-save into a free numbered slot labelled with its core before this session can write, and say so ("Your old auto-save is kept in slot N; it loads on FCEUmm"). The other option is to set `autoSaveHeldFor` and tell the player.

### IOS-03 · Tester feedback still goes to the retired address
HIGH · CONFIRMED · `Feedback.swift:28`; `.kiro/steering/owner-rules.md:76-78`
- Evidence: `FeedbackDestination.email = "idkplswrk@gmail.com"`. Since fd51c47 (today), owner-rules says "Support and feedback email: ContinuumDev2c2b@gmail.com … The old address is retired: it must not appear anywhere in the repository". Every Send in build 180 opens Mail addressed to the old inbox. Outside this area, the old address is also in README.md:52, STATUS.md:208/322/389/944, TESTING.md:282, HANDOFF.md:63 and docs/MANIC_PARITY.md:178.
- Fix: set `FeedbackDestination.email` to `ContinuumDev2c2b@gmail.com` and replace every other occurrence in the same push. The rule's "history included" would need a history rewrite, which conflicts with the no-force-push practice. That is for the orchestrator to raise, not to do silently.

### IOS-04 · The feedback and crash-report forms open fully folded on a fresh install
MEDIUM · CONFIRMED · `SettingsScreen.swift:1062-1068, 1149`; `Feedback.swift:174, 224, 235, 249, 287, 303`
- Evidence: `SettingsSection` renders its content only when `SettingsFolds.shared.isOpen(title)`. That is false for every title while `forceClosed` is set (it returns `openedDespiteForce.contains(title)`), and `forceClosed` is set on any install that never saved a fold, upgraded installs included. The feedback sheet's cards ("WHAT'S THIS ABOUT?", "WHAT HAPPENED", "HOW'S IT RUNNING?", "ANYTHING WRONG?", "YOUR MESSAGE", "SENT WITH IT") are `SettingsSection`s. Only Send stays visible. "Close all" in Settings also closes them. The owner reinstalls before every build, so they will see this every time.
- Fix: add a `foldable` flag (default true) and pass `false` from Feedback.swift, or use a plain card view there. Apply "start closed" only to the Settings screen.

### IOS-05 · Back up to Files copies deleted things back into the app
MEDIUM · CONFIRMED · `CloudSync.swift:1078-1089`; `crates/emulator-bridge/src/sync/mod.rs:221-224, 402`
- Evidence: the backup is `runSync` (two-way) against the in-app staging folder (`Application Support/SyncStaging/Continuum Sync`). For a file missing locally, `decide` downloads it unless `propagates_deletion`, which is true only for save-state slots and covers (sync/mod.rs:395, 402). A game, battery save, player save, Flash/J2ME save, manual, Amiibo or skin file deleted since the last backup comes back on the next Back up. After one "with games" backup, "Delete every game" (`SettingsScreen.swift:735`) is undone by the next Back up, while the games' save states go to `Deleted/`.
- Fix: make the staging run one-way: mirror local to staging, never download into the app (filter out Download/ConflictKeepRemote actions in a mirror mode). The alternative is to rebuild the staging folder from scratch each time; APFS clones keep that cheap.

### IOS-06 · Restore always lets the backup's copy win
MEDIUM · CONFIRMED (ran `pack`/`unpack` on Linux) · `BackupArchive.swift:102-103, 130, 245-292`; `sync/mod.rs:376`; `CloudSync.swift:1231, 1236-1241`; `sync/mod.rs:1101-1131`
- Evidence: every zip entry is dated 1980-01-01 (time 0, date `0x21`), and `unpack` never sets dates, so restored files carry the time of the restore (a 2001 file came back dated now; harness in `/projects/sandbox/.cont-scratch/ios-review`). With no history (`forgetHistory()`), every file on both sides goes to the "newest wins" branch, `local_wins = (l.mtime_ms, l.size) >= (r.mtime_ms, r.size)`, and the backup always wins. The phone's newer save moves to `SyncStaging/Continuum Sync/Conflicts/`, which Files can't show. The code comment and the Rust test `a_restore_shaped_like_bretts_backup_brings_everything_down` assume backup files keep their old dates (`mtime 1_000` against a local `90_000`). The fix in 70d228b only works because the backup always wins anyway.
- Fix: write real dates (DOS time/date plus a 0x5455 extended-timestamp extra field) and set them with `setAttributes` on unpack. Then decide what Restore should mean (backup wins, or newest wins) and say it on screen. If the backup wins, put the phone's replaced copies somewhere visible (for example `Documents/Before restore/`). Fix the Rust test's dates.

### IOS-07 · Restore reports "restore finished" after a failed merge, and accepts any zip
MEDIUM · CONFIRMED · `CloudSync.swift:1248-1251, 1290-1315, 1200-1210`
- Evidence: the follow-up `{ ok in … finishRestore(…) }` ignores `ok`. `finishRestore` sets `pendingSettingsKey` and writes "restore finished: N skin(s), M save state(s)" over the "sync folder failed: …" line. A zip with no `Continuum Sync` folder is used as the backup root (`source = unpacked`), after the in-app copy has already been deleted.
- Fix: pass `ok` through; on failure keep the failure line and don't set `pendingSettingsKey`. Before deleting anything, check that the unpacked root has at least one known folder (`Settings/`, `SaveStates/`, `Battery/`, …), and refuse with a plain message if not.

### IOS-08 · Back up says "saved" even when files failed to copy into it
MEDIUM · CONFIRMED · `CloudSync.swift:1455, 1137-1145`
- Evidence: the export runs on `outcome.refusal == nil`. A run with per-file `errors` (for example no space while copying games) still exports, and "backup saved as … in …" replaces the line that named the errors.
- Fix: pass `outcome.errors` into `exportBackup`. Either refuse, or say "Backup saved, but N files couldn't go in (first: …)".

### IOS-09 · A damaged .zip crashes the app during Restore
MEDIUM · CONFIRMED (ran it) · `BackupArchive.swift:201, 227-231`
- Evidence: the ZIP64 extra-field reads are bounded by the field's own `len`, not by `extraEnd`/`dir.count` (`if size == 0xFFFF_FFFF, q + 8 <= e + 4 + len { size = u64(dir, q) }`). A 120-byte zip with an oversized extra length crashed `unpack` in `Data.subscript` (SIGILL / signal 4) instead of throwing. `dirStart + dirSize` can also overflow. Restore runs in `Task.detached`, so the whole app dies, and the file header promises "every failure is a thrown error shown on screen".
- Fix: bound every read by `min(extraEnd, dir.count)`, use overflow-checked adds for offsets and sizes, and throw `BackupArchiveError("this backup is damaged")`. Keep a host harness with damaged samples.

### IOS-10 · The default NES core is an optional download with no fallback
MEDIUM · CONFIRMED · `ContinuumApp.swift:787-791, 1016, 4011-4014`; `SettingsScreen.swift:616-617`
- Evidence: `mesen2_libretro_ios.dylib` is in `fetch-buildbot-cores.sh names` (optional; `package-ipa.sh` drops a missing one and CI only warns). If it is absent, every NES launch stops with "mesen2_libretro_ios.dylib is missing from the bundle". Nothing switches to FCEUmm, although ff750f5 and the CORES note say "FCEUmm the fallback". Build 180 has it, so this is latent.
- Fix: make mesen2 a required dylib in the ios.yml verify step, or fall back to FCEUmm in `CoreCatalog.core(for:)` when the default's dylib is not in Frameworks.

### IOS-11 · The cBoy Super Game Boy picture is squashed under "Auto (Super Game Boy)"
LOW · CONFIRMED · `ContinuumApp.swift:4795-4797`; `cores/continuum-gb/src/options.rs:78`; `cores/continuum-gb/src/libretro.rs:593, 475-480`
- Evidence: the 8:7 check in `activePictureAspect` lists only `sgb/sgb2/sgb_pal`. `auto_sgb` with an SGB cartridge runs as SGB at 256×224 while the app draws 10:9, about 3% narrower. (Also noted, SUSPECTED: the aspect is recomputed only when SwiftUI re-renders, so a live change of `cboy_sgb_border` may keep the old shape until something else redraws.)
- Fix: expose the core's `retro_get_system_av_info` aspect through the engine and use that. At minimum, treat `auto_sgb` plus an SGB-flagged cartridge (header 0x146 = 0x03) as 8:7.

### IOS-12 · An old Amiibo is re-tapped by itself in later sessions
LOW · CONFIRMED (reading) · `Peripherals.swift:671-698`; `scripts/patches/azahar-libretro-camera-and-amiibo.patch` (`ContinuumPollAmiibo`)
- Evidence: Swift writes `continuum-amiibo.bin` and `.stamp` and never removes them. The patch's `static std::string seen` is empty at every start, so the last stamp is "new" and gets loaded the next time any game searches for a tag. Success is also decided by `line.contains("tapped")` (`!line.contains("not tapped"), line.contains("tapped")`, 677), so a rejected dump whose file name contains "tapped" passes.
- Fix: delete both files when a session starts and ends (Swift side), or seed `seen` from the existing stamp when the core loads. Return a flag from the engine instead of matching text.
- Overlap: same bug as RUST-03 (MEDIUM).

### IOS-13 · Restore never checks CRCs, and a cut-short deflate stream passes
LOW · CONFIRMED · `BackupArchive.swift:276-290, 324`
- Evidence: there is no CRC comparison anywhere in `unpack`. The `last && !finished && piece.isEmpty` exit ends a stream with no END marker and no error, so bit damage or a short download restores as a silently wrong save.
- Fix: CRC each entry as it is written out and compare it with the central directory. Throw on a mismatch or on a missing END.

### IOS-14 · Restore deletes the in-app Conflicts/ and Deleted/ archives
LOW · CONFIRMED · `CloudSync.swift:1213`
- Evidence: `try fm.removeItem(at: root)` runs on the in-app `Continuum Sync` before the move. The Settings note (1937-1944) promises a deleted save state is "moved to Continuum Sync/Deleted rather than removed".
- Fix: carry `Conflicts/` and `Deleted/` over into the restored root, or move them to Documents.

### IOS-15 · A second Back up tap drops the first run's export
LOW · CONFIRMED · `CloudSync.swift:1078-1089, 1404-1413`
- Evidence: `backUpToFiles` sets `afterSync`, and only then does `runSync` see `isSyncing` and set `afterSync = nil`, which discards the running backup's export too. The button isn't disabled while "Working...".
- Fix: return early in `backUpToFiles` when `isSyncing`, and disable both buttons while it is true.

### IOS-16 · The packed zip stays in tmp until the next backup
LOW · CONFIRMED · `CloudSync.swift:1100-1110, 1137`
- Evidence: `ContinuumBackupOut` is removed only when the next export starts. With games it is library-sized.
- Fix: remove it in `exportFinished`, for both saved and cancelled.

### IOS-17 · Streaming unzippers refuse the backup zip
LOW · CONFIRMED (Java) / SUSPECTED (iOS Files) · `BackupArchive.swift:102`
- Evidence: stored entries with flag `0x0808` and data descriptors. Python `zipfile` and `unzip -t` pass; Java `ZipInputStream` says "only DEFLATED entries can have EXT descriptor". Whether iOS Files opens it is untested.
- Fix: after writing the data, seek back, fill in CRC and sizes in the local header, clear bit 3 and drop the descriptor.

### IOS-18 · The inflate loop has no autoreleasepool
LOW · SUSPECTED · `BackupArchive.swift:295-335` (compare 113, 279)
- Evidence: the two other read loops wrap `read(upToCount:)` in `autoreleasepool`; `inflate` doesn't. A multi-GB backup compressed by Files (the documented path for old folder backups) could grow memory across one entry.
- Fix: wrap each chunk's read and process in `autoreleasepool`.

### IOS-19 · finishRestore can reload skins and states while a game is running
LOW · SUSPECTED · `CloudSync.swift:1290-1312, 1462-1466`; `ContinuumApp.swift:2894-2896`
- Evidence: `onlyGamesLeft` allows launches during the games phase (`allowsGameLaunch = true`). `finishRestore` then calls `reloadSkinsFromDefaults()`, which is documented as "Only called when no game is running", and `saveStates.reloadFromDisk()`.
- Fix: keep `allowsGameLaunch` false until `finishRestore` returns during a restore, or defer it until no game is running.

### IOS-20 · SMB shows raw errors, drops a typed port, and blocks the concurrency pool
LOW · CONFIRMED (text, port) / SUSPECTED (pool) · `RemoteSources.swift:193-198, 407`; `smb_min.c:18-26`
- Evidence: libsmb2's `smb2_get_error` text (e.g. "Session setup failed with (0xc000006d) STATUS_LOGON_FAILURE") is shown as "could not list …: …". `smbHost` keeps only `URL.host`, so `smb://nas.local:1445` connects to 445. The blocking libsmb2 calls (20 s timeout) run in `Task.detached` on Swift's cooperative pool; this is suspected to matter only under load, since imports download one file at a time.
- Fix: map the common failures (wrong login, no server, no share) to short sentences and keep the raw text in the log. Pass the port through to libsmb2. Run the calls on a dedicated `DispatchQueue`.

### IOS-21 · Tester text: a heading that doesn't exist, and documentation-style notes
LOW · CONFIRMED · `SettingsScreen.swift:613, 610-622`; `GameDetailSheet.swift:146`; `CloudSync.swift:1937-1944, 1314`
- Evidence: the note says "pick one under CORE", but the card says "Change the core for this game". The CORES and SYNC FOLDER notes are long paragraphs, and the SYNC FOLDER one still describes only the live folder and "this button". The restore line uses "skin(s)" / "save state(s)", the plural style the 9 Oct review pass removed elsewhere.
- Fix: short, casual rewrites ("Want one game on another core? Open the game and pick it there."). Move the live-folder paragraph under "Live folder (advanced)". Use real plurals.

### IOS-22 · The credits list misses four shipped components
LOW · CONFIRMED · `SettingsScreen.swift:1451-1486`
- Evidence: no VICE x64sc (GPL v2) and no Virtual Jaguar (GPL v3), though both dylibs ship. The bundled Ruffle (MIT/Apache) and J2meJS (GPL-2.0) (`scripts/fetch-players.sh:5-9`) are missing too.
- Fix: add the four entries with licence and source link.

### IOS-23 · A restored icon choice marks the wrong icon and blocks re-picking it
LOW · CONFIRMED · `AppIcons.swift:96, 151-156, 213`; `CloudSync.swift:924`
- Evidence: `continuum.appIcon.alternateName` syncs. `remembered()` falls back to it while iOS shows the primary icon, and `apply` returns early when the "current" icon is tapped.
- Fix: add `continuum.appIcon.` to `excludedPrefixes`, and compare against the live `alternateIconName` in `apply`.

### IOS-24 · Stale code comments
LOW · CONFIRMED · `project.yml:97-99`; `ContinuumApp.swift:3752`
- Evidence: "SMB is not in this build", and a Beetle-specific hard-BIOS rule that 876fe64 removed.
- Fix: update them in the same push.

iOS test coverage gap (from the review): no Swift tests cover any of this, and nothing runs `BackupArchive` in CI. Not tested anywhere: Back up after local deletions, Restore over newer saves, Restore with a refused merge, a damaged or non-Continuum zip, the refused-resume auto-save path, the default NES core missing from the bundle, the feedback sheet on a fresh install, SMB on a phone.

---

## Rust engine, build scripts, CI and Continuum Symbian

Scope: `crates/emulator-bridge`, `scripts/`, `.github/workflows/ios.yml`, `native/continuum-symbian`. The PSP/Vulkan and save work was checked against each core's source at its pinned commit and holds up.

### RUST-01 · Symbian firmware folder can't be reached
HIGH · CONFIRMED · `native/continuum-symbian/continuum_symbian_libretro.cpp:129`; `ContinuumApp.swift:3673-3683, 4024-4028, 3706, 3366-3416`; `native_core.rs:433-450`; `eka2l1_engine.mm:134-160, 153-156`; `TESTING.md:139`
- Evidence: the core reads `<Application Support>/continuum-symbian/firmware` (GET_SYSTEM_DIRECTORY = Library/Application Support). Testers can only reach Documents (UIFileSharingEnabled = Documents only), and the instructions (TESTING S1, the core's own message) send them to `Documents/system/continuum-symbian/firmware/`. `installBiosFromDocuments` copies only the named top-level files in `CoreCatalog.installableFirmwareNames`, and nothing mirrors `Documents/system/**`. `InstallFirmware` never finds a `.rom` or `.vpl`, so `bridge::install_device` never runs and `retro_load_game` returns false. Every Symbian launch fails on every phone. `missingFirmware` has no Symbian entry, so the launch is never stopped early with a clear message. Commit `efb6ee9` ("wire Symbian / N-Gage into the app") only gets this far.
- Fix: copy `Documents/system/continuum-symbian/firmware/` across before a Symbian launch: before `engine.launch` for `continuum_symbian`, copy new or changed files into `<Application Support>/continuum-symbian/firmware/` (or have the Settings install button do it). Add a Symbian entry to `missingFirmware` that names the folder in casual words. Keep TESTING S1 and the core's message pointing at the folder testers can actually see.

### RUST-02 · Symbian's refusal is swallowed
MEDIUM · CONFIRMED · `native_core.rs:1310-1317`; `continuum_symbian_libretro.cpp:126, 132`; `retro_log_shim.c:60-62`; `ContinuumApp.swift:1749-1750, 4677`
- Evidence: the refusal reason is passed on only when the last log line starts with `cBoy: `. Continuum Symbian logs a refusal written for people at ERROR level, the shim stores it, and the filter drops it. Testers read `launch failed on continuum_symbian: Game.sis: the game file was refused: retro_load_game rejected the content`. Together with RUST-01, this is the first thing every Symbian tester reads.
- Fix: add one refusal channel for the cores this project owns: either the shim keeps the last ERROR-level line separately, or cBoy and Symbian both start refusals with a fixed prefix such as `refused: `. When there is no reason, fall back to a casual sentence ("This game didn't start on <core name>"), not the libretro function name.

### RUST-03 · An old Amiibo is tapped again after a relaunch
MEDIUM · CONFIRMED · `Peripherals.swift:689-698`; `azahar-libretro-camera-and-amiibo.patch:291-299`; `amiibo.rs:116-118`
- Evidence: Swift writes `continuum-amiibo.bin` and then a fresh ID into `continuum-amiibo.stamp`, and never deletes either file. `ContinuumPollAmiibo` compares the stamp with `static std::string seen`, which starts empty. After any relaunch the stamp on disk looks new, so the first 3DS game that looks for a tag gets the previous session's Amiibo without a tap, even a different game days later. The tester text "A tap that arrives earlier is kept and applied then" only promises that within one session.
- Fix: in `do_load_game`, set `seen` to the stamp already on disk when a game loads, so only taps made during this session count, or delete the stamp when a session starts.
- Overlap: same bug as IOS-12 (LOW).

### RUST-04 · What a game writes to an Amiibo is lost
MEDIUM · how it happens is CONFIRMED, the impact is SUSPECTED · Azahar `nfc_device.cpp:84, 261-320` at pin `065c922`
- Evidence: `NfcDevice::LoadAmiibo` remembers the path it loaded, and `Flush()` writes the game's changes back to that path, which is the scratch `continuum-amiibo.bin`, not the user's file in the Amiibo folder. The next tap overwrites it, so saves to the figure never reach the user's library or backups. Real data loss is SUSPECTED: Azahar only writes back for decrypted ("plain") dumps or when the Amiibo keys are present; without keys the tag is write-protected.
- Fix: load the user's own file (put the real path in the stamp, line 2, and have the patch load it), or have Swift copy `continuum-amiibo.bin` back to the tapped file when the session ends, if it changed.

### RUST-05 · Symbian ignores pause
MEDIUM · SUSPECTED (never run on a phone) · `eka2l1_engine.h:28`; `emu_bridge.h:73-76, 94-95` (pin `661031f`); `continuum_symbian_libretro.cpp:184`
- Evidence: `RunFrame` is empty. The emulator runs on the OS and graphics threads that `bridge::start` creates and plays sound through its own cubeb output. The bridge's `pause()`/`resume()` are never called (CONFIRMED by searching). When the player pauses, opens the menu or goes back to the Library, the Symbian game keeps running and making sound until `retro_unload_game`. Continuum's mute, volume and fast-forward can't reach it.
- Fix: call `bridge::pause()`/`resume()` from the app's pause (or when `retro_run` hasn't been called for a few frames, then `resume()` on the next `retro_run`), and send the sound through libretro (`audio_batch_cb`) instead of cubeb if the bridge allows it.

### RUST-06 · Symbian reinstalls the game on every launch, on the main thread
MEDIUM · SUSPECTED · `eka2l1_engine.mm:186-194`; `emu_bridge.h:150-153`
- Evidence: `Boot()` installs the package each time, even if it is already installed, then matches it by name. Reinstalling a `.sis` rewrites every file the package ships, so packages that ship starting save or settings files would overwrite the player's progress. It all runs on the main thread (Swift's `EngineHost.launch` is `@MainActor`), but the bridge says to "call from a background thread" (`install_ngage_file`), and `install_device` is just as heavy for a 100 MB firmware dump. TESTING S1 already says "give it a minute", which means the screen is frozen for that minute.
- Fix: install each package once (remember its path, size and date in `data_dir_`) and do it off the main thread.

### RUST-07 · cBoy is always rebuilt, yet it is in the shared cache key
MEDIUM · CONFIRMED · `scripts/build-core.sh:487, 1630`; `.github/workflows/ios.yml:266, 317-324`
- Evidence: `continuum_gb` has `IOS_PIN=""`, and `ios_core_is_cached` refuses to reuse any core without a pin. So cBoy is rebuilt on every Mac run, `ios_note_rebuilt` writes `cores-rebuilt.txt` every time, and "Save the built cores" saves a new 72 MB cache entry on every build. The cache list shows three entries with the same source hash `6299259a…`, saved on 11 Oct at 11:05, 11:13 and 12:16. Still, `cores/continuum-gb/src/**` and `Cargo.*` are in `CORE_SOURCES_HASH`, so every cBoy edit forces the other 17 from-source cores to rebuild for nothing. This is why builds 177 and 180 took about 47 minutes (REPORT.md §1).
- Fix: drop the cBoy globs from `CORE_SOURCES_HASH`, and either give cBoy a stand-in pin (a hash of its source that `ios_core_is_cached` can match) or leave it out of `cores-rebuilt.txt`. (Optional, from the review: move `native/continuum-symbian/**` into Symbian's own manifest line so a Symbian edit stops rebuilding Azahar and PPSSPP.)

### RUST-08 · cBoy refusals can show an unrelated line
LOW · CONFIRMED · `cores/continuum-gb/src/libretro.rs:587, 585`; `cart.rs:305`
- Evidence: `.ok()?` at libretro.rs:587 throws away `Cart::new`'s reason ("cartridge type $XX is not supported yet"), so the screen shows whatever cBoy logged last. For example, an unsupported cartridge loaded with force-colour on is refused with the reason "Game Boy cartridge run in Color mode (option)".
- Fix: log `Cart::new`'s error, and only pass on lines that are refusals.
- Overlap: same root as CBOY-07.

### RUST-09 · A graphics failure is called "the game file was refused"
LOW · CONFIRMED · `gfx/moltenvk_device.rs:95, 105`; `native_core.rs:1326-1331`
- Evidence: `finish_deferred_install` returns "the Vulkan device is not ready (MoltenVK did not start)", and it is wrapped as `InvalidContent`. The tester reads "the game file was refused: graphics setup failed: …" for a problem that has nothing to do with the file.
- Fix: return a `Graphics` error with a casual line instead.

### RUST-10 · PSP frame markers flood the activity log
LOW · CONFIRMED · `feedback.rs:163-167`; `bridge.rs:759, 763, 818, 830`
- Evidence: `feedback::frame_phase` is called four times per frame and stays on for every PSP session, writing about four `frame: N <phase>` lines every 120 frames (about 120 lines a minute). After about 2.5 minutes, the 300-line in-memory log, the attached log and the 50-line crash body hold nothing but frame markers, and the launch, sync and save lines are pushed out.
- Fix: log only the first frames (the first few hundred), or keep the latest marker in one slot (a small separate file) that is overwritten each time.

### RUST-11 · The clock-footer trim applies to every system, and the first flush removes the footer
LOW · CONFIRMED · `bridge.rs:1798-1802, 1833`; `SaveStates.swift:908`
- Evidence: the trim in `restore_battery_save` drops an extra 44 or 48 bytes for every core, not only Game Boy. The "unchanged" check in `persist_battery_save` compares save RAM against the whole file, footer included, so they never match. The first periodic flush always rewrites the file without the clock footer, even if the player never saved. An imported mGBA/VBA save loses its clock data as soon as the game opens. That only matters if the save is later exported to another emulator.
- Fix: limit the trim to gb/gbc, and keep the footer when writing back, or skip the rewrite when the file's first `size` bytes (the RAM part) already match.
- Overlap: see CBOY-01's alternative fix.

### RUST-12 · Symbian's files are never refreshed after an update
LOW · SUSPECTED · `eka2l1_engine.mm:75-79`; `emu_bridge.h:67-71`
- Evidence: the core's files are unpacked once (the bridge also "copies … on first launch"), so a later build with new shaders or patch files keeps the old ones.
- Fix: put a version or hash (or the files' size) in the `.continuum-staged` marker and unpack again when it changes.

### RUST-13 · Symbian shutdown order
LOW · SUSPECTED · `eka2l1_engine.mm:224-233`
- Evidence: the last picture is cleared before `bridge::shutdown` stops the graphics thread. A frame being copied at that moment can put the picture back, so the next game can open on the last game's picture, which is exactly what the comment says this prevents. The hook pointer is a plain global that one thread writes and another reads.
- Fix: shut the bridge down first, then clear the picture, and make the hook pointer `std::atomic`.

### RUST-14 · Symbian never uses its JIT
LOW · CONFIRMED (owner rule) · `scripts/build-core.sh:495`
- Evidence: EKA2L1 has the dynarmic JIT and the bridge has `set_jit_enabled`, but build-core.sh says it is "compiled in but never selected". The owner's rule is that every core with a JIT uses it when JIT is available.
- Fix: turn on dynarmic when `GET_JIT_CAPABLE` says yes, after RUST-01 is fixed.

### RUST-15 · The Amiibo stamp file is opened every frame
LOW · CONFIRMED · `azahar-libretro-camera-and-amiibo.patch:59, 291-299`
- Evidence: `ContinuumPollAmiibo` runs from `retro_run` and opens the stamp file 60 times a second on the 3DS emulation thread, which already runs without JIT.
- Fix: check every ~30 frames, or check the file's date.

### RUST-16 · CMake and ninja downloads are not pinned
LOW · CONFIRMED · `scripts/build-core.sh:1271-1278`
- Evidence: the Symbian build downloads CMake 3.31.6 with `curl` and no checksum (1271-1276), and runs an unpinned `brew install ninja` (1278). Every buildbot core, by contrast, is pinned by sha256.
- Fix: check the CMake tarball's published sha256.

### RUST-17 · The cBoy CI step's "SKIPPED" promise is false
LOW · CONFIRMED · `.github/workflows/ios.yml:111-116`; `cores/continuum-gb/tests/roms.rs:487-488`
- Evidence: the step says missing test ROMs are reported as SKIPPED and treats a failed download as a warning, but `scx_low_bits_take_effect_on_line_72` reads its Mealybug ROM with `unwrap()`, so a network hiccup turns the build red (also in REPORT.md §4c).
- Fix: make that test skip like the others.

### RUST-18 · The VMU path comment cites the wrong source
LOW · CONFIRMED (comment only) · `crates/emulator-bridge/src/import/saves.rs:65`
- Evidence: the comment names `core/stdclass.cpp get_writable_vmu_path`. At flycast's pin `59ed35a`, the path is built in `shell/libretro/oslib.cpp:67` (`getVmuPath`). The path itself, `<system>/dc/vmu_save_A1.bin`, is right.
- Fix: fix the comment.

### RUST-19 · Jargon in text testers can see
LOW · CONFIRMED · Azahar pipeline patch (core settings section); `peripherals/amiibo.rs:140-143`
- Evidence: the Azahar option is called "Shader Compile (Continuum)", and its description says "new graphics pipelines compile in the background…". The Amiibo tap line shows the tag's 16-digit hex id.
- Fix: reword both, e.g. "Smoother loading (may pop in)" / "Wait for graphics (may stutter)", and drop the hex id from the status line.

Rust test coverage gap (from the review): untested are `moltenvk_device.rs::apple`, the EKA2L1 engine, whether the Symbian firmware folder matches what Swift exposes, the Azahar camera/Amiibo patch (replay across launches, write-back), non-cBoy refusals reaching the screen, and building the cBoy library on Linux CI.

---

## cBoy integration (`cores/continuum-gb`, paths under `cores/continuum-gb/src/` unless stated)

Every finding was checked by reading the code. Six were also reproduced with a scratch-only test (`/projects/sandbox/.cont-scratch/cboy-review/crate/tests/review_check.rs`). No traced path loses or overwrites a `.srm`.

### CBOY-01 · Cartridge clock never saved
HIGH · CONFIRMED · `libretro.rs:196-238, 910, 931`; engine `bridge.rs:1783-1824` (also `:1794-1802`, `:1783-1864`); `options.rs:106`
- Evidence: cBoy keeps the MBC3/HuC3 clock only in the 24-byte `RETRO_MEMORY_RTC` block (`libretro.rs:70-72`, `:196-211`) and expects the frontend to write it back after `retro_load_game`. `persist_battery_save` and `load_battery_save_file` only touch `MEMORY_SAVE_RAM`. `MemoryRegion::Rtc` exists (`saves_api.rs:30`), but no Swift file calls `readMemory`/`writeMemory`/`memorySize`. Imported mGBA/VBA `.srm` clock footers (44/48 bytes) are trimmed off. Scratch test: an MBC3+TIMER+RAM+BATTERY cart gives `SAVE_RAM 32768 bytes, RTC 24 bytes`, and the `.srm` never contains the clock. The clock survives only inside the auto-save state: it is frozen while the game is closed, and it resets to day 0 whenever the state is missing or refused (any `library_version` change refuses it, `SaveStates.swift:1031-1036`). Pokémon Gold/Silver/Crystal lose day/time/weekday tracking (**likely**, from reasoning). The "Real time keeps the clock running while the game is closed" text (`options.rs:106`) is false, and so is "Phone clock … at each start" for the day count (Phone mode only sets seconds, minutes and hours, `libretro.rs:232-237`). Moving a game between mGBA and cBoy loses the clock both ways.
- Fix: persist the 24-byte RTC block next to the `.srm` (`<game>.rtc`, in sync and backup, same triggers: exit, background, timer) and write it into the region right after `retro_load_game`, before the first frame. CloudSync already treats `.rtc` as a save extension (`CloudSync.swift:81`). Alternatively, carry the clock inside save RAM as the standard 48-byte footer: the engine then stops trimming it, and the cBoy state tag must be bumped. **Fix CBOY-03 and CBOY-09 in the same push**, or the fix brings them to life. Until then, the "Cartridge clock" text is false.

### CBOY-02 · Phone motion never reaches cBoy
MEDIUM · CONFIRMED · `libretro.rs:68, 656-660, 322-326`
- Evidence: `ENV_GET_SENSOR_INTERFACE = 25`, but libretro.h defines it as `25 | RETRO_ENVIRONMENT_EXPERIMENTAL` (0x10019), and that is the only value Continuum matches (`input/sensors.rs:33`, dispatched at `input/mod.rs:50`). The call falls through to `_ => false` (`native_core.rs:709`). Scratch test: cBoy sends `[67, 27, 10, 9, 23, 25, 15, …]`, never `0x10019`. `Core.sensors` stays `None`, every `read()` returns 0.0, and CoreMotion never starts. With the defaults "Phone motion" / "Phone rotation" (`options.rs:193-198`), Kirby Tilt 'n' Tumble, Yoshi Topsy-Turvy and WarioWare: Twisted! see a still console. Also, `start_sensors` sets `sensors_started = true` before checking anything and runs once per session, so switching tilt to "Phone motion" mid-game never turns the sensors on.
- Fix: change `ENV_GET_SENSOR_INTERFACE` to `25 | 0x10000`, and re-run `start_sensors` when the tilt or turn option changes, not just once per session.

### CBOY-03 · HuC3 clock scrambled by the real-time catch-up
MEDIUM (latent) · CONFIRMED · `libretro.rs:196-229, 722-729`; `cart.rs:60-94`
- Evidence: for HuC3, `write_rtc_block` packs `huc3.minutes`/`days` into `rtc.regs[0..4]`, and `read_rtc_block` then runs MBC3 `advance_seconds(elapsed)` over those bytes (byte 0 as 6-bit seconds wrapping at 60, and so on) before unpacking them. Scratch test: day 5 10:00, closed for one hour, comes back as minutes 536, day 6 (should be 660, day 5). `retro_reset` restores only `cart.rtc`, so Restart zeroes the HuC3 clock and its 256-nibble clock memory. This is latent today (the block is never written back) and goes live with the CBOY-01 fix.
- Fix: give HuC3 its own catch-up (add elapsed minutes to `huc3.minutes`/`days`) instead of ticking MBC3 seconds. Keep `huc3` across Restart (`libretro.rs:722-729`).

### CBOY-04 · Boot ROM / BIOS / custom palette can't work on a phone, and the text is false
MEDIUM · CONFIRMED · `options.rs:89, 91, 115`; `libretro.rs:608, 159-162, 597-617`; app `ContinuumApp.swift:703-709, 3380, 3673`
- Evidence: `system_file` looks in `<system>/cboy/<name>` then `<system>/<name>`. The system folder is Application Support (`ContinuumApp.swift:3673-3683`, passed at `:4017-4027`), which Files can't see. `installBiosFromDocuments` copies only `installableFirmwareNames`, and cBoy's `CoreSpec` has `biosNames: []`. So `cboy_boot_rom`, `cboy_gba_bios=file|file_skip` and `cboy_dmg_palette=custom` silently do nothing on every phone, while the descriptions point testers at `<system folder>/cboy/dmg_boot.bin` and "hex" colours. `fffbd2f` replaced the correct "Without the file the game starts directly" and the log line "starting without a boot ROM" with "own short startup" wording, but that startup was removed in `ee2c2f9`. `build_machine` already starts directly when no file is found. (REPORT §4b already found the false sentence and the dead intro code; the above is new.)
- Fix: put back "Without the file the game starts directly" and the old log line. Either add cBoy's file names to its `biosNames` (with a `cboy` subfolder) or hide those three choices. Take the `<system folder>/cboy/...` paths out of tester text and delete the dead intro code (`gb.rs:17-19`, `:141-178`, test `:232`).

### CBOY-05 · Game Boy ROM run as an Advance game
MEDIUM · SUSPECTED · `gba/mod.rs:20-22`; `libretro.rs:544`
- Evidence: `is_gba_rom` is `rom.len() >= 0xC0 && rom[0xB2] == 0x96`, checked before anything else. On a GB cart `$B2` is ordinary code and `$96` is `SUB (HL)`. Scratch test: a valid GB header with that byte set gives a 240x160 Advance machine. How many real ROMs hit this is unknown (no ROM set scanned). When it happens the game can't be played on cBoy and no option forces Game Boy. In the reverse case, Advance homebrew without `$96` falls into the GB path, where `cutoff_reason` can read `$148` as a size and show "This file is cut off…".
- Fix: also require that the Game Boy Nintendo logo is absent at `$104` (`$104-$133`), or that the GBA header complement at `$BD` checks out, before choosing the Advance.

### CBOY-06 · "Next start" options that apply live
LOW · CONFIRMED · `libretro.rs:252, 258, 401-421`; `options.rs:134-136, 169-171`
- Evidence: `cboy_sample_rate` says "Next start", but `apply_live` writes it into the running APU on every option change. The app reads the rate once at launch (`bridge.rs:419-424`; `SET_SYSTEM_AV_INFO` is a no-op at `native_core.rs:683`), so 32768 Hz mid-game plays as 48 kHz, too high and crackling (**likely**). The border option likewise switches 160x144↔256x224 live while the renderer keeps the launch shape (`bridge.rs:437`), so the picture is squashed or stretched by about 3%.
- Fix: read the sample rate and border choice once, at load, as their text promises, so the audio rate and picture shape match what the app read at launch.

### CBOY-07 · Unsupported cartridge shows a code-like refusal
LOW · CONFIRMED · `libretro.rs:587`; `cart.rs:291, 305`
- Evidence: `Cart::new(...).ok()?` throws away "cartridge type $0B is not supported yet" and "the file is too small…". With nothing logged, the engine falls back to "retro_load_game rejected the content" (`native_core.rs:1310-1318`), and the tester sees "the game file was refused: retro_load_game rejected the content" (`ContinuumApp.swift:1749-1750`). MMM01, MBC6, TAMA5 and bootleg mappers all hit this.
- Fix: log the `Cart::new` error before returning, worded casually and without `$0B`-style hex.
- Overlap: same root as RUST-08.

### CBOY-08 · Serial log grows forever
LOW · CONFIRMED · `bus.rs:35, 395`
- Evidence: every internally clocked transfer pushes a byte onto `bus.serial_out`, which only Blargg-style tests read. With no partner a transfer completes instantly. Scratch spin loop: 131,527 bytes in 60 frames (about 470 MB/hour, enough for iOS to kill the app). Ordinary link polling is about 216 KB/hour. Run-ahead hides it (`#[serde(skip)]` resets on restore).
- Fix: cap `serial_out` or fill it only in tests; nothing in production reads it.

### CBOY-09 · Real-time catch-up stops at 10 days
LOW (latent) · CONFIRMED · `cart.rs:89-94`
- Evidence: `advance_seconds` caps the catch-up at 864,000 seconds. Scratch test: closed 30 days, it advances exactly 10, and nothing sets the day-carry bit. The loop also runs up to 864,000 steps on the first frame.
- Fix: add the elapsed days arithmetically, setting the day-carry bit past 511, instead of looping at most 864,000 seconds. (CBOY-01 asks for this in the same push.)

### CBOY-10 · "Light sensor" sunlight choice is always dark on iPhone
LOW · CONFIRMED · `options.rs:199-201`; `libretro.rs:315-316`
- Evidence: the engine refuses `ILLUMINANCE_ENABLE` (`sensors.rs:352`), so `read(6)` is always 0 and Boktai sits in the dark. The text "the phone's light sensor if it has one" can't be true on any iPhone.
- Fix: remove the choice on iOS, or fall back to the 50% level when the frontend refuses illuminance.

### CBOY-11 · Game Boy Camera claims a phone picture it never gets
LOW · CONFIRMED (claim) / SUSPECTED (offset) · `cart.rs:180-186, 231, 650` (reader `:628`)
- Evidence: the comments say `camera.image` holds "the last picture from the phone", but nothing writes it, so every photo is four fixed vertical bands. `capture` writes the 128x112 tiles from offset 0 of RAM bank 0. Pan Docs (from memory, not re-checked) places the image at `$A100-$AEFF`, so it may land 256 bytes early and overwrite `$A000-$A0FF`.
- Fix: correct the comments and keep the camera out of tester-facing "supported" lists. Check Pan Docs and write the capture at `$A100` instead of `$A000`.

### CBOY-12 · Jargon and debug switches in Core settings
LOW · CONFIRMED · `options.rs:67, 81-84, 103, 115, 135, 152, 157-161, 223-238`
- Evidence: the app shows every category as a normal group (`CoreSettingsScreen.swift:434-443`), so testers see a "Debug" group of 13 layer switches ("Sprites" off makes a game look broken), plus "DirectSound", "Output capacitor … offset and all", "aliasing", "256x224", "four colours as hex" and "handshake". "Game Boy Color revision" offers five revisions, A-E, that behave identically; revision 0 only changes a DIV value for GB-only carts on a Color (`gb.rs:115-118`). Owner rule: technical read-outs only behind (i) and Settings → Technical details.
- Fix: rewrite the text in plain words, and hide the Debug group and the five identical Color revision choices from testers.

cBoy checked and cleared (no finding): no dangling save-RAM pointer after state load/Restart/run-ahead (`native_core.rs:1170-1180`); state size within the 1 KB slack; null frames from frame skip handled (`native_core.rs:125-150`); `state_decode` bounds allocations; GBA save-type changes that don't fit are refused without overwriting the `.srm` (`SaveStates.swift:1651-1660`); both state layouts pinned by `tests/fuzz.rs:114-129`.
