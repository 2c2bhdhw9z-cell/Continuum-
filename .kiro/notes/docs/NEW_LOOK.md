# The second look ("New")

Settings → **LOOK** (the first card under the Settings title, never folded) → **Current** or **New**.
Current is the look the app has always had and is the default, so an existing install is unchanged.
The switch applies at once; no restart or reinstall. The choice is stored in UserDefaults under
`continuum.ui.look.v1` (`current` or `new`).

No branded theme names, on purpose (Brett's call): the setting is "Look", the choices "Current" and
"New". Code names: `ContinuumLook.current` / `ContinuumLook.newLook`, `LookStore`, `NewLook*` views.

## The mockup

Brett had one mockup, of Home only (sent in chat 19 Oct 2026; the image file was not reachable from
the build box, so it is described here instead of committed):

- Dark charcoal background, gold/yellow accent.
- Header: CONTINUUM wordmark with a rainbow ring logo; a glass capsule with search, +, profile.
- "LAST PLAYED" hero: title, "SYSTEM • core • 1h 45m played", angled box art, gold **Resume** and
  glass **Game Hub** buttons.
- SYSTEMS: a horizontal carousel of consoles with game-count badges; the selected one has a gold
  glow border; "View all".
- "Jump Back In": a row of recent games with "Last played: 2d ago" (the mockup also showed
  "45% complete" bars; the app has no progress data, so those are not shown).
- A floating translucent tab bar: Home, Library, Favorites, Settings, gold when active.

## What each screen does in the new look

`LibraryShell` draws both looks. It branches only where they differ (page background, header,
Home, tab bar, the Library's system chips), so everything else is the same code in both and
neither falls behind. `ShellPalette.accent`, `onAccent`, `background`, `card` and `cardStroke`
follow the look; `danger` is red in both. `RootView` forces dark while New is on.

| Screen | File | Notes |
|---|---|---|
| Home | `NewLookShell.swift` | Hero = last played game (from `PlayHistory`), or the newest import labelled RECENTLY ADDED when nothing was played yet. Resume when the game has an auto-save, otherwise Play. Systems carousel (drawn console shapes, no console photos or logos), Jump Back In (only played games), Recently Added. |
| Library | `LibraryShell.swift` + `NewLookSystemChips` | The All Games tab, titled Library, with system chips (All + each system with a count) above the same layouts, card sizes, select mode and shuffle as Current. Tapping a console on Home opens Library filtered to it. |
| Favorites | `LibraryShell.swift` | The same tab as Current, in the New colours. |
| Settings | `SettingsScreen.swift` | The same screen; cards, toggles and buttons take the gold accent and charcoal. Delete buttons stay red. |
| Game card (Game Hub) | `NewLookShell.swift` `NewLookHubHeader`, used by `GameDetailSheet.swift` | Angled cover over a blurred copy, system, "Runs on <core>", play time, last played, Play/Resume, heart, close. Everything below (core picker, save states, cheats, manual, achievements, skin, cover art, delete) is the same card as the current look. |
| In-game | `PlayerScreen.swift`, `NewLookPause.swift` | Top bar: back, eye, title, rewind/fast-forward (held), a gold pause button, the same ⋯ menu. The pause button pauses and opens a dark glass sheet: Resume, Save state, Load last, Save slots, Reset, Swap screens, Cheats, Skin, Controllers, Online play, Use as cover, All functions, Feedback, Tech details, Quit to library. Closing it any way resumes the game if the menu paused it. During online play the game is not paused (that would stall the other phone). |

## Play history (new data)

`PlayHistory` (`LookTheme.swift`) records, per game file name, when it was last opened and how long
it stayed open in the foreground (`continuum.library.lastPlayed.v1`, `continuum.library.playSeconds.v1`,
epoch seconds and seconds). It is hooked to `EngineHost.activeEntry`, which every launch and every
stop goes through. A single stretch over 8 hours is not counted. It starts empty: games played
before this build show no play time until they are played again. Both looks record it; only the
new look shows it.
