# A second "New" look (charcoal and gold, motion, colour panels, pause menu) beside the unchanged Current look

These twelve commits add Settings > LOOK > Current | New. The New look has its own Home, Library, Favorites, Settings styling and Game Hub header, an in-game pause menu, a colour panel, and a motion layer, plus play-time recording, console photos and even cover frames. One UserDefaults key, read through a non-isolated `LookStore`, switches between the looks. `ShellPalette` and `isNew` branches pick the look at each point where they differ, and `RootView` re-identifies `LibraryShell` when the key changes. No compile risk and no Current-look regression turned up. The problems are in the pause menu's resume contract, the cost of the motion layer, and how the new data travels through Cloud Sync.

Watch for: the pause menu's **Resume does nothing if the game was paused before the menu opened**, and choosing Save slots / Cheats / Skin / etc. **un-pauses the game behind the next sheet** (confirmed). The New-look library runs **dozens of always-on per-frame animations** (animated shadows on every card and console tile, ~30 sweep clocks in Settings) (likely battery/thermal hit; none of it runs during gameplay). Each **ColorPicker drag tick writes UserDefaults and re-renders all of Settings** (likely). **Play history syncs as an ordinary setting** and gets overwritten after a reinstall (likely). The New look's floating tab bar **lets taps through to covers underneath** (likely).

**Verdict**: NEEDS_CHANGES

## High-level view

No type-check risk was found. CI built every commit through `f8c5195` against the iOS 16.0 target, where an unguarded iOS 17/18 API is a compile error. The one later commit, `97ee938`, was pushed `[skip ci]` and has never been built, but it is a clean deletion.

The Current look keeps its look and behaviour. Every shared edit is one of three things: an `isNew` branch with the old view in the else-arm, a colour that resolves to the same value in Current, or an inert overlay. The only visible change is the intended LOOK card at the top of Settings. Play-time recording runs in both looks.

The pause menu pauses only when it was the one to pause, and Quit leaves cleanly through `stopSession`. Two parts of its contract are broken: Resume can't undo a pause the menu didn't make, and second-level choices resume the game before their sheet opens. The library and all its motion are out of the view tree during play, and nothing new redraws per frame. Motion does run in the pause sheet (live during online play) and in the in-game Feedback sheet.

The motion layer never stops while the library is open. Breathing animated shadows sit on every cover frame and console tile, even when their colour is `.clear`. On top of that come a blur per card, a fluid Canvas in the hero, a ring that spins by default, and one TimelineView sweep per title, about 30 of them in Settings' non-lazy ScrollView.

Colour picks write straight into `NewLookColors`. The system ColorPicker writes continuously during a drag, so each tick saves UserDefaults and redraws Settings, `LibraryShell` and the tab bar.

Persistence is UserDefaults only. It is written at game open, game close and app background. Missing keys mean Current and defaults, so existing installs are unaffected. Cloud Sync replaces every `continuum.` key wholesale at the next launch, which loses play time. `LookStore` is built before the pending cloud settings land, so a restored look shows one launch late. No timers, leaked observers or main-actor violations were found.

The preview workflow can't block `ios.yml`. It is still a macOS job with no concurrency group and no inner timeouts, it runs alongside `ios.yml` for every motion edit, and one run already hung for its full 30 minutes.

<details>
<summary>Issues (12)</summary>

1. **Resume no-op after a prior pause** (confirmed) — `finishPauseMenu` only resumes when `pausedForMenu`. Make `.resume` resume whenever `host.paused && !host.netplayLive`.
2. **Pause menu un-pauses before follow-on sheets** (confirmed) — Save slots/Cheats/Skin/Functions/Controllers/Feedback open over a running game. Keep the game paused and resume when that sheet is dismissed.
3. **Always-on animated shadows on every card and tile** (likely) — `NewLookBreathe` runs per card and per console tile, even with `.clear`. Make the glow static (or animate opacity of one pre-blurred layer), and skip it when glow is off or the tile is unselected.
4. **Sweep/swatch clocks never pause** (likely) — about 30 TimelineViews at 30 fps in non-lazy Settings, plus the hero fluid and the spinning ring. Run a sweep only on screen (or as one shared clock), cut fps during rest phases, and honour Low Power Mode.
5. **ColorPicker drag writes per tick** (likely) — every tick saves UserDefaults, bumps `version` and re-renders Settings and `LibraryShell`. Hold a draft colour and commit on dismiss or after a short debounce.
6. **New-look Library re-sorts per render** (likely) — `LibrarySort.sorted` (`displayTitle` in the comparator) and `NewLookSystemTray.counts` run on every `LibraryShell` body. Cache them the way the shelves are cached.
7. **Floating tab bar lets taps through** (likely) — the New look drops the hit-blocking wash, so taps beside or under the capsule reach covers and can launch a game. Add `.contentShape(Rectangle())` (or a near-clear fill) to the New-look bottom chrome.
8. **Play history overwritten by Cloud Sync** (likely) — `lastPlayed.v1`/`playSeconds.v1` are replaced wholesale at the next launch after a download. Merge them on import (max `lastPlayed`, max `seconds`), the way the skin keys are merged.
9. **Look restored one launch late** (likely) — `RootView` builds `LookStore.shared` before `EngineHost.init` runs `CloudSync.applyPendingSettings()`. Apply pending settings first (e.g. in `ContinuumApp.init`), or re-read the store after applying.
10. **"Resume" label ignores auto-resume setting** (confirmed) — Hero and Game Hub say Resume whenever an auto-state exists. Also check `saveStates.resumesAutomatically`.
11. **Preview workflow cost/hang** (confirmed) — macOS job, double macOS run with `ios.yml` on motion edits, no concurrency group, and one run hung 30 min. Add `concurrency` with cancel-in-progress, `timeout` on the simctl calls, and a lower `timeout-minutes`; consider dispatch-only.
12. **`97ee938` never built** (confirmed) — `[skip ci]` skipped both workflows. Dispatch an iOS build before handing out an install.

</details>

<details>
<summary>Details</summary>

## What CI has and hasn't compiled

`iOS` succeeded on `0d6b7f0`, `11657aa`, `d3684f5`, `7a40c0b`, `003f1c0`, `c55141c`, `d0ee59a` and `f8c5195`, with `deploymentTarget iOS: "16.0"` (project.yml:13-14). Availability violations are compile errors at that target, so MeshGradient, two-argument `onChange`, `sensoryFeedback`, `@Observable`, `scrollPosition` and `symbolEffect` cannot be present unguarded in those trees. A sweep of the added lines agrees. The only gated call is `presentationBackground` behind `#available(iOS 16.4, *)` (NewLookPause.swift:226-231). Every `onChange` uses the single-argument closure, which is deprecated on 17 and produces warnings only.

`97ee938` carries `[skip ci]`, and the Actions API shows no run of either workflow for it (confirmed). It deletes `NewLookSparkleShape`/`NewLookSparkleMark`, which are no longer referenced in any `.swift` file including the preview. It also replaces the branch in `EmptyLibraryNotice.newBody` with a single `Image(systemName: cond ? … : …)`. Nothing in it should fail, but the next install would be the first build of this tree.

## Pause menu: what pauses, what resumes

```
gold pause ─▶ openPauseMenu()            (PlayerScreen.swift:321)
               if !netplay && !paused { togglePause(); pausedForMenu = paused }
               else                   { pausedForMenu = false }
sheet closes ─▶ finishPauseMenu()        (PlayerScreen.swift:336)
               .quit  → leavePlayer() → stopSession() (paused = false)    ✓
               save / loadLast / reset / swap / cover / details run paused
               if pausedForMenu && paused { togglePause() }     ← resume gate
               +200 ms: slots / cheats / skin / functions / controllers /
                        netplay / feedback sheet opens
```

The resume gate is keyed only on whether the menu did the pausing (confirmed). If the game was already paused when the gold button was tapped, `pausedForMenu` is false. That happens with a floating button set to `menu` (`PadAppAction.menu` → `togglePause`, ContinuumApp.swift:5488-5497) or any other `togglePause` caller. The big "Resume" button then dismisses the sheet and leaves the game paused. The New-look top bar dropped the Current play/pause toggle (PlayerScreen.swift:443-447 has no equivalent in `newLookTopBarItems`), so the only way back is that same floating button. To the player it looks like a frozen game.

The second gap is ordering (confirmed). It is deliberate per the code comment, but surprising in play. Every choice that opens another sheet resumes the game first and opens the sheet 200 ms later. A player who paused to pick a save slot, run a RAM search, change skin or remap controllers gets the game running behind that sheet. In the Current look the same sheets also open over a running game, but there the player never asked for a pause. Keeping `pausedForMenu` alive until the follow-on sheet's dismissal would match what the menu promises.

Reset on a Flash/J2ME game runs `restartWebPlayer()`, which nils `webPlayer` before the resume gate. The gate's `togglePause()` then hits `guard running` and writes "pause ignored: no game is running" to the status line, until the restarted session sets `paused = false` (confirmed, cosmetic).

During online play the menu doesn't pause, by design. Its header still runs a `NewLookFluid` Canvas, a `NewLookFloat` with its own sparks clock, a sweep and a flowing rim (NewLookPause.swift:146-181), all over a live game. The emulator's display link shares the main run loop with them (possible frame-pacing hit; worth one netplay test). Outside that sheet, the only New-look motion during play is the three `SettingsSection` heading sweeps in `FeedbackSheet`, over a running game. The library itself is unmounted while `activeEntry != nil` (ContinuumApp.swift:6267-6290), and the telemetry fields are not `@Published` (ContinuumApp.swift:1896-1898).

The hero and the Game Hub label their button "Resume" whenever an auto-state exists (NewLookShell.swift:227, GameDetailSheet.swift:71). They ignore `saveStates.resumesAutomatically`, so with auto-resume off in Settings, "Resume" starts the game from the beginning (confirmed).

## Motion budget in the New-look library

Nothing in the motion layer is gated by visibility, Low Power Mode, or whether the effect is even visible. Reduce Motion is the only off switch. A default-theme New-look Home runs all of this at once (likely heavy; needs an Instruments pass on a phone):

```
header   NewLookWordmark ring        repeatForever rotation (ringSpins defaults ON)
hero     NewLookGlow → NewLookFluid  Canvas 30 fps, always on (not tied to "Moving gradient")
         title sweep, box float + NewLookSparks clock, Play button flowing rim
SYSTEMS  every NewLookSystemTile     NewLookBreathe(color: selected ? gold : .clear)
shelves  every NewLookGlowCard       NewLookCoverFrame: blur(28) backdrop + static shadow
                                     + NewLookBreathe animated shadow (even when glow == nil)
         each section title          its own NewLookClock sweep, 30 fps, also during rest phase
tab bar  NewLookTabPill              clock 20 fps + breathe
```

The animated shadow is the expensive part. `NewLookBreathe` (NewLookMotion.swift:180-198) animates shadow radius and opacity with `repeatForever`. That forces a re-rasterised shadow of each card's content, blurred backdrop included, on every display frame for every visible card. NewLookMotion.swift:445 applies it with `glow ?? .clear`, and NewLookShell.swift:1013 applies it to every unselected tile with `.clear`, so "Cover glow: Off" still pays. The `.ultraThinMaterial` header, glass buttons and tab bar sit over this constantly changing content, so their backdrop blur is recomputed as well. "Moving gradient" adds a full-screen 11-mass `NewLookFluid` behind everything.

Settings has the most clocks. Each New-look `SettingsSection` heading has a sweep (SettingsScreen.swift:1117), and there are ~26 sections in a non-lazy `VStack` (SettingsScreen.swift:76). Then add:

- the LOOK title and the five colour-panel titles (NewLookTheme.swift:333);
- a 20 fps clock per gradient swatch (NewLookTheme.swift:482; 7 presets plus saved blends);
- flowing rims on the picked chips;
- the preview's fluid Canvas.

That makes about 40 TimelineViews ticking, including the ones scrolled off screen.

## Colour picks: one write and one full redraw per drag tick

`NewLookColors.save` (LookTheme.swift:492-500) writes UserDefaults and bumps `version` on every property set. The ColorPicker bindings (NewLookTheme.swift:176-189, 270-293) assign every value the system picker reports, and UIColorPickerViewController reports continuously during a drag. The `stops[index] != hex` guard only drops exact duplicates. Each drag tick therefore does three things (likely):

- a UserDefaults write;
- a re-render of all of `SettingsScreen` (which now observes `NewLookColors`, SettingsScreen.swift:30) and of `LibraryShell`;
- a rebuild of the tab bar from scratch (`.id(newLookColors.version)`, LibraryShell.swift:834-836), with a spring over the whole colour panel.

## New-look Library sorting per render

The existing shelf code is cached because an n-log-n sort per redraw was "the first thing to fix for scrolling". The New-look `allGamesTab` reintroduces exactly that (likely). It calls `looks.librarySort.sorted(...)` inline on every `LibraryShell` body (LibraryShell.swift:1026-1031), with `GameMetadata.displayTitle` (tag stripping plus patch lookup) called twice per comparison. It also rebuilds `NewLookSystemTray.counts` over the whole library on each pass. `LibraryShell` re-renders on any host publish, and now also on `LookStore`, `NewLookColors` and `PlayHistory`.

## Floating tab bar hit-testing

The Current bottom chrome's material and wash exist, per its comment, so that "a tap on the bar (the home-indicator strip under the tabs included) … never reaches a cover passing underneath it". The New look drops that background (LibraryShell.swift:860-867). Its capsule is inset 16 pt at the sides and `max(bottomInset - 10, 10)` at the bottom (NewLookShell.swift:184-186), and those empty padding regions are not hit-tested. Scroll content runs underneath, so a tap beside or below the capsule lands on a `NewLookGlowCard`, whose tap is `host.launch(entry:)` (likely). The fix is `.contentShape(Rectangle())` on the New-look branch of `bottomChrome`.

## Play history, colours and Cloud Sync

The new keys:

- `continuum.ui.look.v1`
- `continuum.ui.newLook.librarySort.v1`
- `continuum.ui.newLook.*` (strings; nil means default)
- `continuum.library.lastPlayed.v1` and `continuum.library.playSeconds.v1` (`[String: Double]`, keyed by file name)

`librarySystem` is memory only. Play history is written in `began` and in `closeStretch` (game close and `didEnterBackground`) (LookTheme.swift:192-229).

Cloud Sync exports every `continuum.` key (CloudSync.swift:1794-1798). `applyPendingSettings` replaces those keys wholesale on the next launch; only the three skin keys are merged (CloudSync.swift:1823-1840). For play history that loses data (likely). The owner deletes and reinstalls every build, and the cloud copy is downloaded and marked pending during launch 1 of the fresh install. Play during that first session is written locally, then overwritten by the older cloud copy on launch 2. With two phones, each apply replaces one phone's totals with the other's. A max-merge per game name alongside `mergedSkinKeys` fixes both cases.

`applyPendingSettings` runs inside `EngineHost.init` (ContinuumApp.swift:3720). `RootView`'s `@ObservedObject private var looks = LookStore.shared` (ContinuumApp.swift:6253) is evaluated when `RootView()` is built, before SwiftUI runs the `@StateObject` autoclosure that creates `EngineHost` (ContinuumApp.swift:6250). So `LookStore` reads the look and sort keys before the cloud values land (likely), and the restored look appears one launch late. Nothing is corrupted, because `LookStore` writes only on change. `NewLookColors` and `PlayHistory` are first touched after `EngineHost.init`, so only `LookStore` is affected.

## New-look preview workflow

`newlook-preview.yml` runs on dispatch, or on a master push that touches `NewLookMotion.swift`, `tools/newlook-preview/**` or the workflow itself. It has no concurrency group of its own and a read-only token, so it can't block or cancel `ios.yml`. It does cost a `macos-latest` runner. A `NewLookMotion.swift` edit also matches `ios.yml`'s `native/**`, so motion tweaks pay for two macOS jobs, and back-to-back pushes run preview jobs in parallel.

`run.sh` puts no timeout on `simctl bootstatus`, `install`, `launch`, `io screenshot` or `recordVideo`. The `d0ee59a` run sat in "Build, run and film the preview" from 23:51 to 00:21 and was killed by `timeout-minutes: 30` (confirmed via the Actions API). The successful run took 11 minutes and kept a 33 MB artifact for 90 days.

</details>

<details>
<summary>File map</summary>

- `native/ios/LookTheme.swift` — new: `LookStore` (look and sort keys), `LibrarySort`, `PlayHistory`, `LookPickerCard`, `NewLookColors` (theme store, hex helpers).
- `native/ios/NewLookShell.swift` — new: New-look backdrop, header tools, floating tab bar, Home/hero/shelves, `NewLookGlowCard`, SYSTEMS tray and tiles, Game Hub header, buttons.
- `native/ios/NewLookMotion.swift` — new: clock, fluid Canvas, sparks, float, breathe, sweep, flowing rim, press/toggle styles, appear, tab pill, `NewLookCoverFrame`. Sparkle removed in `97ee938`.
- `native/ios/NewLookTheme.swift` — new: colour panels (accent, covers, ring, background), ColorPicker bindings, `ThemeSwatch`, corner radius helper.
- `native/ios/NewLookPause.swift` — new: `PauseChoice`, `NewLookPauseSheet`, 16.4 glass background.
- `native/ios/PlayerScreen.swift` — New-look top bar items, pause sheet presentation, `openPauseMenu`/`finishPauseMenu`.
- `native/ios/ContinuumApp.swift` — `activeEntry` didSet → `PlayHistory`; `RootView` observes `LookStore`, `.id(look)`, forced dark for New.
- `native/ios/LibraryShell.swift` — New-look branches for page, top bar, tabs, Library header/tray/sort, Favorites by console, tab bar; observes colours and history.
- `native/ios/LibraryCards.swift` — `ShellPalette` made look-aware (`red`, `gold`, `charcoal`, `danger`, `card`, `onAccent`); New-look `EmptyLibraryNotice`.
- `native/ios/LibraryExtras.swift` — New-look console headers and glow cards in `ConsoleSections`.
- `native/ios/SettingsScreen.swift` — LOOK card at top; New-look section headings, sweeps, springs, press style, accent-tinted segmented thumb; console-photo credit.
- `native/ios/GameDetailSheet.swift` — New-look Game Hub header; destructive colour via `danger`.
- `native/ios/project.yml` — five new sources added.
- `native/ios/CONSOLE_PHOTOS.md` — photo sources and licences.
- `.github/workflows/newlook-preview.yml`, `tools/newlook-preview/*` — simulator build of NewLookMotion.swift with stubs, screenshots and video artifact.

Full diff: `git diff 39f8187..97ee938 -- native/ios tools .github`

</details>
