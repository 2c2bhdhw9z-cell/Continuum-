# Bug audit, 7 October 2026

Six parallel code reviews (library, data, player, skins/settings, Rust engine, build) plus build
142's own log. This was the tally the owner asked for. Every HIGH and
CRITICAL item below was re-read in the code before it went on this list.

Confidence: **CONFIRMED** = traced in the code and definitely wrong. **SUSPECTED** = the code looks
wrong, but a phone or a real build is needed to be sure.

"Not built yet" = in code on master that no build has compiled. Builds 142 and earlier don't have it.

## Where each item stands (re-checked 9 October 2026, against the code on master)

| # | State | Note |
| --- | --- | --- |
| 1 | Fixed | Skin unzip caps each file at 64 MB. |
| 2 | Fixed | Frame counters are no longer published; the library does not redraw per frame. |
| 3 | Fixed | note() no longer copies into the host. |
| 4 | Fixed | Confirmed on the phone in build 144. |
| 5 | Fixed | Settings export skips while a downloaded copy waits, and a fresh install no longer wins by being newer. |
| 6 | Fixed | Skins get a stable id from their identifier and systems; the index merges. |
| 7 | Fixed | Each skin entry decodes field by field; a bad entry no longer empties the list. |
| 8 | Fixed | The games switch moved to a key that survives and is set by Choose a folder, with games. |
| 9 | Fixed | Unknown keys are left alone (adoptLegacy knownSystems). |
| 10 | Fixed | Both pointers are handed back, and the host frees a slot by its run or write address. |
| 11 | Fixed in code, needs a phone call to confirm | Sound is rebuilt on coming back to the front. |
| 12 | Fixed | Leaving the front releases fast forward and rewind; the hold catches cancels. |
| 13 | Fixed | Reports include the previous launch's log. |
| 14 | Fixed | The JIT line swaps the part number for the phone's name. |
| 15 | Fixed | Re-import stages the new copy, then swaps; a failed copy keeps the old one (confirmed on phone, 144). |
| 16 | Fixed | Only a sync of saves blocks a launch; a games copy no longer does. |
| 17 | Fixed | Abandoned .upload temp files are swept from the cloud folder. |
| 18 | Fixed | Arrival stamps follow copy order, newest first. |
| 19 | Fixed | A skin imported for another console no longer becomes its own console's default. |
| 20 | Fixed | Core cache is saved per run with a fresh key, before xcodebuild. |
| 21 | Fixed | Cache prefix includes xcodebuild -version. |
| 22 | Fixed | Every network call in the update check is guarded per repository. |
| 23 | Fixed | The release step un-drafts an adopted release. |
| 24 | FIXED IN THIS PASS | A logged-out session retries the stored token when a game starts. |
| 25 | Still present | A landscape-only skin is stored as the upright face. Left alone: changing how skins are stored needs phone testing with such a skin. |
| 26 | Fixed | Pieces are walked to any depth. |
| 27 | FIXED IN THIS PASS | The unreachable-folder check now also runs per top-level folder, so many skins or games no longer dilute it. |
| 28 | FIXED IN THIS PASS | This device's files compare exact times; the 2 s slack is kept only for the cloud side. |
| 29 | FIXED IN THIS PASS | The picture conversion checks the buffer length and draws black instead of crashing. |
| 30 | Fixed | Same fix as 2. |
| 31 | Fixed | The engine-change observer is scoped to the game's own audio engine. |
| 32 | Fixed | The backup prompt waits for the crash prompt, on its own view. |
| 33 | Can't tell without a phone | Bar height is still 84 pt; scrolling was confirmed fine on 144. |
| 34 | Fixed | Help text says file name. |
| 35 | FIXED IN THIS PASS | Rewind note no longer says nothing to pick; skins note names .manicskin; Reset now asks first. |
| 36 | Fixed | Audio session deactivates with notifyOthersOnDeactivation. |
| 37 | FIXED IN THIS PASS | Flash pauses itself if paused while loading; J2ME sound made while paused starts silent. Needs a phone to confirm. |
| 38 | Fixed | All patches use a bool function type. |
| 39 | Fixed | Release tags use --target github.sha. |

## Things Kiro got wrong this session (said plainly)

| # | What | Why it matters |
| --- | --- | --- |
| K1 | The scrolling "fix" doesn't fix it (bugs 2 and 3) | The library still redraws dozens of times a second, through a second route that was missed |
| K2 | The new sync can't actually bring things back after a delete (bug 5) | Your phone's empty settings beat the cloud's real ones |
| K3 | Skin sync can wipe skins (bugs 6 and 7) | The skin ids it relies on being stable are random |
| K4 | The backup prompt promises your games come back, and they don't (bug 8) | The games switch is wiped with the app, so games never sync |
| K5 | Batch imports show back to front in Recently added (bug 18) | Off-by-direction in the arrival stamps |
| K6 | The core cache can store a set with a core missing, and then never complete it (bug 20) | Builds stay slow until the cache key changes |

## CRITICAL: crash, data loss or build break

1. **A malicious or broken skin file can crash the app.** DeltaSkinImport.swift:1337. The unzip
   trusts the size each file claims and keeps doubling its buffer with no limit, so a zip bomb or a
   false size uses up memory and iOS kills the app. Imports also run on the main thread, so picking
   several large skins freezes the screen. CONFIRMED.

## HIGH: a feature visibly broken

2. **Scrolling, cause A: the whole library redraws 60–120 times a second, even with no game
   running.** MetalCanvas.swift's display link never pauses and hands over frame counters every
   frame. ContinuumApp.swift:5302-5307 writes them into published values unconditionally, which
   tells every screen watching the host to redraw. The library watches the host. CONFIRMED.
3. **Scrolling, cause B: the "fix" was bypassed.** ArtworkStore.swift:2702-2705, `note()`, copies
   every artwork status line into the host's published `artworkLine`. The library watches the host,
   so it still redraws once per cover resolved, which is what the unbuilt fix was meant to stop.
   CONFIRMED. *(Bugs 2 and 3 are why scrolling "still feels the same".)*
4. **On Home, the logo and search bar sit on top of shelf titles once you scroll.**
   LibraryShell.swift:171-180. The bar's background is a fixed gradient that fades to nothing.
   Nothing tracks scrolling, so with the hero art gone there is nothing behind the bar. Visible in
   your screenshot. CONFIRMED.
5. **After a reinstall, the first sync throws away the cloud's settings and cover choices.**
   CloudSync.swift `exportSettings()` writes a fresh, empty settings file stamped "now". With no
   sync history, sync/mod.rs:365 picks the newer file, so the empty one wins, the real one goes into
   `Conflicts/` (which the app never reads) and the empty one is uploaded. Lost: favourites, the skin
   index, Recently-added order, system choices, saved servers, cover choices. CONFIRMED. *(Not built
   yet for skins and favourites; settings were already affected.)*
6. **Syncing skins can delete another device's skins.** Skin ids are random for every import
   (SkinLibrary.swift:345), even though a comment in CloudSync.swift says they're stable. The skin
   index is synced as one whole value and applied by replacing, never merging (CloudSync.swift:1039).
   Two devices, or a reinstall, means one device's skin list overwrites the other's. CONFIRMED.
   *(Not built yet.)*
7. **One unreadable skin entry wipes the whole skin library, and saves the wipe.**
   ContinuumApp.swift:2610. If any single entry fails to decode, the whole list decodes as empty;
   line 2620 then deletes every skin record and line 2622 saves the empty list, which will now sync
   to other devices. The likeliest trigger is a skin list written by a different build arriving
   through sync. CONFIRMED mechanism, SUSPECTED trigger.
8. **The backup prompt says your games come back. They don't.** The "Back up the games too" switch
   is stored under a `continuum.sync.` key (CloudSync.swift:99). That prefix is excluded from sync
   (:746), and the switch is wiped when the app is deleted. So after a reinstall it's off and no game
   downloads. "Choose a folder" never turns it on either. CONFIRMED. *(Not built yet.)*
9. **Skins with no index entry show up as fake consoles.** SkinLibrary.swift:328-335 runs on every
   launch, despite its own comment saying "used once". Any skin file without an index entry becomes
   its own "system" named after a random id, which shows in the skin list as something like
   "SKIN-1A2B…" and fits no console. CONFIRMED.
10. **Dreamcast never gives its fast-mode memory back.** flycast-ios-jit-region.patch:139 releases
    `code_area1`, flycast's own static array, instead of the memory Continuum handed it. jit26.c
    ignores pointers it doesn't recognise (:280-290), so each Dreamcast launch keeps its share
    forever. After roughly ten Dreamcast sessions in one run, every JIT request fails and every JIT
    core falls back to its slow mode. On current iPhones that fallback cannot run code at all.
    CONFIRMED. *(Only matters with JIT on.)*
11. **Audio can stay silent after a phone call until you leave the game.** AudioOutput.swift:429-435.
    A call sets "interrupted", and only the "interruption ended" notice clears it. iOS doesn't
    promise to send that notice, and usually doesn't once the app was suspended. Coming back then
    refuses to restart sound and shows "audio: still interrupted". SUSPECTED, needs a call to test.

## MEDIUM: wrong in some cases

12. **Fast forward or rewind can get stuck on.** PlayerScreen.swift:416. The hold button only
    releases when the finger lifts (`onEnded`). Pulling down Control Center, a notification or a
    call mid-hold cancels the gesture without lifting, so the game keeps running fast or rewinding.
    SUSPECTED.
13. **The feedback activity log is nearly empty.** Feedback.swift:649. A normal report only attaches
    this launch's lines, and the log is restarted on every launch (feedback.rs). Only one kind of
    status message is logged. That's your 51-byte attachment. CONFIRMED.
14. **The JIT line still shows the raw part number** ("iPhone18,2"). jit.rs:436-440 builds it, and it
    shows in the report and in the player's (i) panel. The device-name fix only changed the
    "iPhone:" line. CONFIRMED.
15. **Re-importing a game deletes the old copy before the new one has arrived.**
    ContinuumApp.swift:3903-3906. If the copy fails (full disk, a cloud file that won't download),
    the game is gone. CONFIRMED.
16. **A games sync blocks playing for minutes.** ContinuumApp.swift:4060 refuses to launch a game
    while a sync runs, on the grounds that "it is seconds at most". With games syncing it is
    gigabytes. CONFIRMED. *(Not built yet.)*
17. **Interrupted uploads leave hidden full-size files in your cloud forever.** CloudSync.swift
    `upload()` writes to a hidden temporary name, and cleans it up only if the copy itself fails. If
    the app is closed mid-upload, nothing ever deletes it. With games syncing, each one can be
    gigabytes. CONFIRMED.
18. **Recently added shows each import batch backwards.** ContinuumApp.swift:2816 gives each later
    file a later time, and the shelf sorts newest first (LibraryShell.swift:731). Imports run A to Z,
    so the shelf shows Z to A and features the alphabetically last game. CONFIRMED. *(Built in 141.)*
19. **"Import for" a different console gives the skin's own console the wrong buttons.**
    ContinuumApp.swift:2180-2194. Importing a GBA skin "for N64" also makes it GBA's default, using
    the N64 button mapping. CONFIRMED.
20. **The core cache can be stuck missing a core and slow forever.** ios.yml:242 saves only when the
    cache missed, and entries can't be updated. If an optional core (Dreamcast is the slow one) fails
    on the run that fills the cache, every later build rebuilds that core and can never store it.
    CONFIRMED.
21. **The cache key ignores the compiler version.** ios.yml's key covers three scripts only. When
    GitHub moves its runner to a new Xcode, builds keep shipping cores made by the old compiler until
    an unrelated edit flushes the cache, and that unrelated commit then fails if the new compiler
    has a problem. CONFIRMED mechanism.
22. **The weekly emulator-update check dies on one network hiccup.** check-core-updates.sh:67 is the
    one call with no per-repository guard. Under `set -euo pipefail`, one failure ends the whole run
    and the page isn't updated. CONFIRMED.
23. **A release can be left as an unpublished draft while the build shows green.** ios.yml's retry
    adopts an existing release and re-uploads, but never publishes it if it was left as a draft. The
    newest-build link then keeps serving the previous build. CONFIRMED against `gh`'s own code.
24. **Achievements are off for a whole session if the app opens without internet.**
    Achievements.swift tries the saved login once at launch and never retries. CONFIRMED.
25. **Landscape-only skins are stored as the upright face.** DeltaSkinImport.swift ~451-454. They
    draw letterboxed upright and have nothing for sideways. SUSPECTED.
26. **Skin button images in subfolders never sync.** The sync only handles `pieces/<id>/<file>`, one
    level deep, but skins can put pieces in subfolders. SUSPECTED. *(Not built yet.)*
27. **The "folder unreachable" safety check is weaker now.** sync/mod.rs:423 compares against all
    local files, and skins and games now add many. A half-loaded cloud folder can then remove local
    save-state slots (copies kept in `Deleted/`). CONFIRMED mechanism.
28. **A battery save written within 2 seconds of a sync can be overwritten.** sync/mod.rs:52 allows
    2 seconds of clock slack on local files too. Battery saves don't change size, so an edit in that
    window looks unchanged, and a later download from another device replaces it with no conflict
    copy. CONFIRMED mechanism.
29. **A core skipping a frame can bring back an older frame.** native_core.rs:1349-1354. If a new
    core's format differs, the stale frame can crash the picture conversion (convert.rs has no length
    check). SUSPECTED trigger.
30. **The game screen redraws every frame on the main thread too** (same cause as bug 2). That adds
    battery drain and heat while playing. CONFIRMED mechanism.
31. **The microphone's audio engine resets the game's audio.** AudioOutput.swift:763-769 listens for
    changes from every audio engine in the app, including the microphone's, so mic games can get
    gaps or keep cutting out. SUSPECTED.
32. **Two pop-ups at launch can collide.** The crash report and the backup prompt are both alerts on
    the same screen. SwiftUI shows one, and the other can get stuck. SUSPECTED. *(Not built yet.)*

## LOW: minor

33. The bottom padding (84 pt) is shorter than the real bottom bar (~100 pt), so the last row of the
    grid can't scroll fully clear. SUSPECTED.
34. The Favorites help text still says favourites are remembered "by the game's path on disk". They
    are now remembered by file name. CONFIRMED.
35. Settings text is out of date in two places: rewind says "nothing to pick" right above a picker,
    and the skins note only mentions .deltaskin files. "Reset every system's controls and skins"
    deletes every skin with no confirmation. CONFIRMED.
36. Leaving a game doesn't tell other apps they can resume, so your music doesn't come back by
    itself. CONFIRMED.
37. A paused Flash or J2ME game keeps running with sound if you pause before it finishes loading.
    CONFIRMED.
38. Two core patches call the JIT function through the wrong return type (int instead of bool).
    Harmless on iPhone, would break on Android. CONFIRMED.
39. Release tags point at whatever master was when the build finished, not the commit built.
    CONFIRMED.
40. Compiler warning: Peripherals.swift:42 uses `allowBluetooth`, deprecated in favour of
    `allowBluetoothHFP`. From build 142's log, the only warning in our code.
41. Workflow warning: the GitHub actions used (cache, checkout, upload-artifact) are on Node 20,
    which GitHub is forcing onto Node 24. From build 142's log.

## Not a bug, but you should know

- **Master has app changes no build has picked up.** Four `[skip ci]` commits since build 142: the
  scrolling attempt, the cover flash, the device name, the backup prompt and the cache fix. The
  download link serves 142 without any of them, until a build is run.
- **Checked and fine:** the N64 JIT patch, which was checked against the real upstream code; sync
  creating missing folders; web-player file serving; achievement memory reads; JIT function
  exports; libretro callbacks null-checking everything.
