# Home screen and light mode (13 and 14 October 2026)

On 13 October at 5:30 PM the owner sent a screenshot of the Home screen: the games should scroll
under the four tabs ("make the four tabs hover"), the "● 80 games ^" strip above the tabs should go
("there's already that in the settings, I think"), and players should be able to choose how big the
game cards are. At 5:50 PM they asked for a light mode too, with Settings, Appearance: Dark, Light,
System. Everything here is **Built, untested**: nothing has been on a phone yet.

- U1, the tabs float and the covers scroll under them: `a06aeec` (an earlier session).
- U2, the "80 games" strip is gone: `c3e8bac`, below.
- U3, Settings → Layout → Game card size: `d31c627`, below.
- U3b, light mode and Settings → Appearance: `97e02ef`, below.
- U4 (a fresh review) and U5 (the write-up): added here as each one lands.

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

## U3b: Light mode, and Settings → Appearance (`97e02ef`) — Built, untested

**Settings → APPEARANCE**, its own short section third from the top (right under CORES, which an
older note keeps second): **Dark**, **Light**, **System**, the owner's own words. No sentence
under it. Dark is the default and is exactly how the app always looked (the app used to force
dark), so nobody's app changes until they pick something. System follows the iPhone's own light
or dark setting, and changes with it.

### How it works

- **Applied once, at the root** (`RootView`): SwiftUI's `preferredColorScheme` (nil for System),
  and the same choice on the app's windows (`AppAppearance.applyToWindows`, UIKit's
  `overrideUserInterfaceStyle`, `.unspecified` for System). The window half covers what UIKit
  presents by itself (the Files picker, Mail, the share sheet, a game's manual), and going back to
  System through `preferredColorScheme(nil)` alone has been reported to stick on the old style on
  some iOS versions. The TV's window and the keyboard's are left alone.
- **Live:** the root observes the setting, so every screen redraws the moment it changes, no
  restart. The setting is on the Settings tab, not in a sheet, so no open sheet is left behind in
  the old style.
- **Every sheet follows it now.** Thirteen sheets forced dark on their own
  (`.preferredColorScheme(.dark)` on save slots, cheats, feedback and its drawing screen, the skin
  library and the four skin sheets a game opens, the manual viewer, "Which system?", Import and
  Add a server). That's gone from all of them, so menus and sheets opened from a game follow the
  setting too.
- **One palette, two values each** (`ShellPalette` in `LibraryCards.swift`). Its Dark values are
  the exact colours it always had. Light mirrors them: black where Dark has white, white where Dark
  has black. Two new names, `text` and `background`, replace each `Color.white` used as page text
  and each `Color.black` used as a page or a wash (about 100 lines in 16 files). The teal is a
  deeper teal on white so it still reads, and the smallest grey labels are a little stronger.
- **Saved like the other settings:** `continuum.appearance.v1`, a plain `continuum.` key that no
  exclusion list matches, so it syncs and goes into Back up to Files like Game card size.

### What it looks like in Light

- **Library:** white page, black text. The big cover at the top fades into white, not black, with
  the title and Play on the white under it. The top bar's fade over the cover is white too.
- **The floating tabs:** Apple's light glass under a 60% white wash (the mirror of Dark's 60%
  black), so the black labels and the red selected tab read over any cover passing underneath. The
  pop-up line above them is the same glass.
- **Covers:** the system badge and the favourite star sit on their own dark chips over the art,
  so they stay white-on-dark in both modes, like the art itself. Titles under grid cards, shelf
  titles, counts and list rows are black. Cards get a faint dark edge on white.
- **Settings:** light grey cards on white, black text, red switches. The segmented selectors are
  a faint grey groove with a white glass thumb, like iOS's own light selector.
- **A game's card, save slots, cheats, feedback, skins, import, servers, the manual:** white pages
  with black text. The RAM search's white-tinted buttons take the text colour instead, so they
  can't turn into white words on white buttons.

### What stays dark in all three, on purpose

- **The game screen:** black around the picture, the pad and every button over it, the status
  line over the game, the telemetry, the keyboard, the achievement banner, the online-play line.
  The pad's UIKit view is pinned dark too, so even its two system colours (the orange and teal of
  extra floating buttons) don't shift.
- **The on-screen control editor and the skin editor.** Both show the game screen itself, laid
  out for editing (the pad over the game's place on black, the skin art on black), so they stay
  dark like the game. Each is a full-screen cover with its own `preferredColorScheme(.dark)`, so
  Settings behind keeps the player's choice.
- **The technical block** (behind the top bar's dot, in Settings → Technical details, and behind
  (i) in a game): white monospace on its own dark box everywhere, because the same block is drawn
  over a running game. On a light page it reads like a console.
- **The title cards** a game without a cover gets: they're art, coloured from the game's name.
- **The Flash and J2ME player surfaces:** black, like the game screen. Their pages don't opt in to
  dark or light styling, so nothing in them changes.

### What changed in the code

- `LibraryCards.swift`: `ShellPalette` (every colour with a Dark and a Light value, the
  `dynamic`/`adaptive` helpers, and the Dark values as UIKit colours for the editor's menu
  button), `AppAppearance` (the three choices and `applyToWindows`), and `StatusTint.isEveryday`.
  Four places decided "is this line a problem?" by comparing the colour with the teal (two in
  `LibraryShell`, the player's status line, the import in `ContinuumApp`); they ask `isEveryday`
  now, so a teal with two values can't break them.
- `ContinuumApp.swift`: `EngineHost.appearance`, stored and read back like `libraryCardSize`; the
  root applies it (the window group's old fixed `.preferredColorScheme(.dark)` is gone).
- `SettingsScreen.swift`: the APPEARANCE section, and its text, labels, buttons and selector on the
  palette.
- The other screens: `LibraryShell`, `GameDetailSheet`, `SaveSlotsScreen`, `CheatSearch`,
  `Feedback`, `SkinLibraryScreen`, `GameplayManuals`, `ImportCenter`, `ImportScreen`,
  `RemoteSources`, `ArtworkChooser`, `AppIcons`, `Achievements`, `AppleOverlay`, `CloudSync`,
  `Peripherals`, `ScreenModes`, `InputExtras` (its Settings card only).
- Kept dark: `TouchLayoutEditor` and `SkinEditor` (one line each), `TouchControls` (one line in
  `makeUIView`), `PlayerScreen` (a comment on the technical block, and the `isEveryday` change).
- `docs/PRODUCT_SCOPE.md`: the UI target now says Appearance exists and the game stays black.

### What was checked

- `swiftc -frontend -parse` on all 25 changed Swift files.
- Dark is unchanged, value for value: every Dark value in the palette is the colour that was
  there before (white at 8, 12, 14, 45 and 62%, black at 28%, the same teal); `text.opacity(x)`
  in Dark is the old `Color.white.opacity(x)` and `background.opacity(x)` the old
  `Color.black.opacity(x)`; the selector's new white layer under the glass is clear in Dark.
- No API newer than iOS 15: `Color(uiColor:)` (iOS 15); `UIColor(dynamicProvider:)`,
  `overrideUserInterfaceStyle`, `UIWindowScene.windows`, `UISceneSession.role` and
  `preferredColorScheme` (iOS 13); single-parameter `onChange` and `tint(_:)` as the app already
  uses them. The one new `@MainActor` call is made from `onAppear` and `onChange` in `RootView`'s
  body, the way `onOpenURL` there already reaches the main-actor host.
- Every remaining fixed `Color.white`/`Color.black` in the app was listed and sorted: each one is
  over cover art, over the running game, in one of the two editors, or the root's black letterbox.
- `preferredColorScheme` now appears only at the root and in the two editors.
- Nothing in Rust was touched.

### Phone checks (for TESTING.md, in the owner's words)

1. Open the app without changing anything. It looks exactly like before: dark.
2. Go to **Settings → Appearance** (near the top) and pick **Light**. The whole app turns light
   straight away. Scroll Home: the covers still pass under the tabs, and the tab names are easy to
   read over light and dark covers.
3. Look at the big cover at the top of Home. It fades into white, and the game's name and Play
   under it are easy to read.
4. Press and hold a game to open its card. It's light too, with dark writing. Open **Save
   slots** from it, then close it.
5. Start a game. The game screen is still black around the game, with the same buttons as
   always. Tap **⋯**: the menu is light. Open **Cheats and RAM search**, then **Skin...**: both are
   light. Close them and go back to the Library.
6. In Settings, open **Move the on-screen controls**. The editor stays dark, like the game.
7. Pick **System**. Open Control Centre and switch your iPhone between light and dark (hold the
   brightness slider, then tap Dark Mode). The app switches with it.
8. Pick **Dark** again. Everything's back to how it always looked.

### TESTING.md lines that are out of date because of U3b

None. Its lines about black are about a game's picture, which stays black.

### For the fresh review (U4) to look at hardest

- The window override and `preferredColorScheme` together at the root, and going back to System.
- The two editors staying dark, and the technical block's dark box on light pages.
- The Light opacities (text at 60% and 55% on white) and the deeper teal.
