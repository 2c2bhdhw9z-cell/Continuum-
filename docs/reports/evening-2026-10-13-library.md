# Home screen and light mode (13 and 14 October 2026)

On 13 October at 5:30 PM the owner sent a screenshot of the Home screen: the games should scroll
under the four tabs ("make the four tabs hover"), the "● 80 games ^" strip above the tabs should go
("there's already that in the settings, I think"), and players should be able to choose how big the
game cards are. At 5:50 PM they asked for a light mode too, with Settings, Appearance: Dark, Light,
System. Everything here is **Built, untested**: nothing has been on a phone yet.

- U1, the tabs float and the covers scroll under them: `a06aeec` (an earlier session).
- U2, the "80 games" strip is gone: `c3e8bac`, below.
- U3, Settings → Layout → Game card size: `d31c627`, below.
- U3b (light mode), U4 (a fresh review) and U5 (the write-up): added here as each one lands.

## U2: the "● 80 games ^" strip is gone (`c3e8bac`) — Built, untested

### What the strip carried, and where each piece is now

| What the strip showed | Where it is now |
| --- | --- |
| The game count, "80 games" | **Settings → STORAGE**, first line: "80 games in your library" ("1 game in your library", "No games yet"). New: Settings only had it as "Delete every game (80)" and the technical "library: 80 game(s) of 86 file(s) in Documents" under TECHNICAL DETAILS → STORAGE, which stays. All Games still says "80 titles" and the search box "Search 80 titles...". |
| The dot | Its colour came from the latest status line: teal when all is well, orange when something was skipped, missing or cancelled, red when something failed. The same dot was already in the top bar, next to +, and stays. Each pop-up now carries that colour too. |
| The ^ | It opened the full block of technical text above the tabs. That block was already in **Settings → TECHNICAL DETAILS → Show technical stuff**. In the Library the dot in the top bar still opens and closes the same block above the tabs. |
| "Moved 23 files that aren't games to the Not games folder in Files.", until tapped or the next import | Pops up. |
| The latest status line whenever it read as a problem (orange or red): "Added 3 games. Skipped 41 files that aren't games.", an import that failed, "sync folder failed: ...", "artwork: 12 lookup(s) failed ...", a BIOS install that failed, a game that couldn't start | Pops up. Every status line is also in the technical block (its second line) and in the activity log that Send feedback attaches. |
| Every status line, while Show technical stuff was on | The technical block shows it (its second line). |
| A plain import that went fine: only the count going up | New: it pops up "Added 3 games.". An import of only skins, cheats, saves or manuals pops up what the import said about them. |

### How the pop-up works

- It sits over the content just above the floating tabs, never on them, in the tabs' own glass and
  dark wash, so it reads the same over any cover passing underneath.
- It goes away by itself after 5 seconds, or 10 for a problem (the same times as the status line
  over a game). A tap takes it away at once.
- One at a time. Up to two wait behind it (the newest are kept), and the same line is never queued
  twice (an import's line arrives both as the Library's line and as the status line).
- The time only counts while the Library can be seen. While a game's card, the Files picker,
  "Which system?" or an alert covers it, the line waits, so a message written from under a sheet is
  still there when the sheet closes, as the strip's line was.
- Everyday lines ("surface ready", "deleted Tetris.gb") don't pop up, exactly as the strip left
  them out.
- Problem lines are the app's status lines word for word, so some are technical (for example
  "picker cancelled: no files chosen", which the strip showed in orange too). Rewording them is the
  shorter-text job's.
- Small and known: Wi-Fi transfer imports one file at a time, so with the Import screen open for a
  big upload, closing it shows "Added 1 game." once. The Import screen lists each file as it
  arrives, and Settings → STORAGE has the total.

### What changed in the code

- `LibraryShell.swift`: the strip (`statusStrip`, `stripText`) is gone. The pop-up (`popUpLine`)
  takes `EngineHost.libraryNotice` and every status line `StatusTint` doesn't call everyday, on
  appear (once per Library, for what was written before it was on screen) and as they change. The
  technical block stays in the bottom chrome behind the top bar's dot.
- `ContinuumApp.swift`: `importFiles` hands the Library its result line ("Added 3 games.", the
  casual skipped line, or the summary when only a disc's tracks went in); a failed import's summary
  pops up as a problem. It no longer clears a waiting Not games line at the start; a move found by
  the same rescan follows the import's line. Comments that described the strip were updated.
- `ImportCenter.swift`: `NotGames.importLine` leaves out its second sentence when nothing was
  skipped. Its only other caller always passes a skipped count, so that line reads as before.
- `SettingsScreen.swift`: STORAGE's game count line.
- `docs/PRODUCT_SCOPE.md`: the UI target listed "a thin status strip above the tab bar"; it now
  says the owner had it taken out and where things went.

### What was checked

- `swiftc -frontend -parse` on all five changed Swift files.
- No new kind of API: the material, `background(_:in:)`, `strokeBorder`, the transitions,
  `withAnimation`, single-parameter `onChange` and `Task.sleep` are all used elsewhere in the app
  already (iOS 15 and older); `accessibilityHint` is iOS 14. The one call into another file,
  `EngineHost.topmostViewController()`, is made from a `@MainActor` function, the way Feedback,
  ArtworkStore and CloudSync already call it.
- No Rust was touched, so the engine tests can't be affected.

### Phone checks (for TESTING.md, in the owner's words)

1. Open the app. The "● 80 games ^" line above the tabs is gone.
2. Go to **Settings → Storage**. The first line says how many games you have.
3. Tap **+** and add a game. A little message pops up just above the tabs ("Added 1 game.") and
   goes away by itself after a few seconds. Add another and tap the message: it goes away at once.
4. Add a zip that has a README or LICENSE file in it. The message says "Added … games. Skipped …
   files that aren't games."
5. Tap the little dot at the top right, next to **+**. The block of small technical text opens
   above the tabs. Tap the dot again to close it.

### TESTING.md lines that are out of date because of U2

- Line 63, "Next install (after 181)" step 1: "A line at the bottom says files that aren't games
  were moved to the Not games folder." The line now pops up just above the tabs when the app opens
  and goes away after a few seconds.
- Line 69, step 2: "a line says "Added … games. Skipped … files that aren't games."" It's a pop-up
  now.
- Lines 762-763, "The single most useful thing you can do": "in the Library behind the thin line
  above the tabs". It's behind the little dot at the top right, next to +, and in Settings →
  Technical details → Show technical stuff.
- Line 785, Test 1 step 3: "Tap the thin line of small text just above the tabs at the bottom".
  Now: tap the little dot at the top right, next to +.
- Line 792, Test 1 "You should see": "A thin line of small text above the tabs. Tapping it opens
  the diagnostic block." Now: a little dot at the top right, next to +; tapping it opens the
  diagnostic block.

## U3: Game card size (`d31c627`) — Built, untested

**Settings → LAYOUT → Game card size**, right under "All Games: Grid / List": Extra small, Small,
Medium, Large. Medium is the default and exactly the size the cards always were, so nobody's app
changes until they pick something. No line of explanation under it; the four choices say it all.

### What each size does

Covers in a grid row (All Games and Favorites), iPhone in portrait:

| Size | 375-428 pt wide phones | 430 and 440 pt (Plus, Pro Max) | Grid card | Home shelf card |
| --- | --- | --- | --- | --- |
| Extra small | 6 | 7 | 50 × 67 pt | 64 × 85 pt, about 5 on screen |
| Small | 4 | 4 | 78 × 104 pt | 92 × 123 pt, about 3½ |
| Medium (today) | 3 | 3 | 110 × 147 pt | 124 × 165 pt, about 2¾ |
| Large | 2 | 2 | 160 × 213 pt | 160 × 213 pt, about 2 |

In landscape (852 pt) the grid holds 14, 9, 7 and 4. Shorter cards also mean more rows fit
down the screen, on Home as well as in the grids.

- **Home:** every shelf's cards (Recently added and each system's shelf) change width, height and
  the gap between them. The big featured cover at the top doesn't change.
- **All Games and Favorites:** they share one grid, so both follow the setting. The List layout
  has no covers to resize and stays as it is.
- **Text scales with the card:** the system badge, the favourite star, the corners, the title
  under a grid card (10, 11, 12 and 14 pt, still two lines at most) and the line under it. At Extra
  small the line under the title ("NES · USA · 24.0 KB") is left out; the badge already says the
  system. On a small favourite the badge shrinks to fit beside the star instead of running under
  it. On a game with no cover, the title on its coloured plate scales too, so at Extra small a Home
  shelf still says which game it is (in a small grid card the title is right under the cover, so
  the plate leaves it out there, the same way it always did under 92 pt).
- **Live:** the Library observes the setting, so Home and the grids redraw at the new size at once,
  with no restart.
- **Saved like the other settings:** `continuum.library.cardSize.v1`, next to the Grid/List
  choice's key. It's a plain `continuum.` key that no exclusion list matches, so it travels in
  `Settings/defaults.plist` with sync and with Back up to Files, the same as Grid/List.

### What changed in the code

- `LibraryShell.swift`: `LibraryCardSize` (the four sizes and every number for each, Medium's
  copied from the old code), and the grid and the shelves read `host.libraryCardSize`.
- `LibraryCards.swift`: `CoverCard`, `SystemBadge` and `ShelfRow` take the size, defaulting to
  Medium. The badge got one line and a 0.7 shrink limit, which changes nothing for a badge that
  fits (every badge at Medium does).
- `GameArtwork.swift` (`ArtPlate`) and `ArtworkStore.swift` (`CoverArtView`): a `captionScale` for
  the plate's caption, defaulting to 1. The hero, the list rows, the game's card and the artwork
  chooser keep the default.
- `ContinuumApp.swift`: `EngineHost.libraryCardSize`, stored and read back like `libraryLayout`.
- `SettingsScreen.swift`: the one new row in LAYOUT.

### What was checked

- `swiftc -frontend -parse` on all six changed Swift files.
- Medium matches the old numbers one for one: 124 pt shelf cards 12 apart; grid columns 104 to
  150 pt, 12 apart and 16 down; 110 pt grid cards; corners 10; badge 10 pt bold rounded, padding
  6 and 3, corner 6, inset 6; star 11 pt, padding 5, inset 6; title 12 pt semibold, two lines;
  the line under it 10 pt, one line, no shrinking (`minimumScaleFactor(1)` is SwiftUI's default);
  plate caption at scale 1. The one new rule, `showsCaption: !showsLabel || width >= 92`, gives
  the same answer as the old `width >= 92` for both Medium cards (124 on a shelf, 110 in the grid).
- The size table, taken out of the file, built with swiftc and run through the adaptive grid's
  arithmetic for nine screen widths (375 to 440 pt, and 852 in landscape). That's where the
  covers-per-row numbers above come from. Away from Medium, every card fits inside its column on
  every width. (Medium's 110 pt card in a 106 pt column on a 375 pt phone was already like that.)
- No new kind of API. `GridItem.adaptive`, `lineLimit`, `minimumScaleFactor`, `.font(.system(size:
  weight:design:))` and `background(_:in:)` were already used in these files. Each new parameter
  is a defaulted `var` after the existing ones, so every caller in other files still works:
  `ArtworkChooser` calls `ArtPlate`, and `GameDetailSheet` and the hero call `CoverArtView`.
- No Rust was touched.

### Phone checks (for TESTING.md, in the owner's words)

1. Go to **Settings → Layout**. Under All Games there's **Game card size**, set to Medium. Home
   and All Games look exactly like before.
2. Pick **Extra small**, then go to **All Games**. About twice as many covers fit in a row (6
   instead of 3), and you can see a lot more games without scrolling. The names under the covers
   are smaller but still readable.
3. Go to **Home**. The covers in every row are smaller too, so more rows fit on screen. The big
   cover at the top is the same as before.
4. Try **Small** (4 in a row) and **Large** (2 in a row). Each change shows right away, no
   restart.
5. Star a game and look at it at **Extra small**. The little star and the system name at the top
   of the cover don't overlap.

### TESTING.md lines that are out of date because of U3

None. Its one Library layout line, "Library layout, Grid and List | Works" (line 708), is still
true.
