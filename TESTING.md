# What to test, and what to tell me

Last updated 4 October 2026, for **build 121 or newer**. The newest install is always on the
[Releases page](https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest) as
`Continuum-<number>.ipa`.

This file has two halves. **The list right below is the one to work through.** Everything in it is
built and no phone has confirmed it yet. Further down is a record of what already works, kept only
so a new build that breaks an old thing gets noticed.

**How to report:** say the test number and what happened. If something goes wrong, a screenshot of
the whole screen is the best thing you can send. The line of small text near the top of the player
is the app's only "error log", so it usually explains the problem.

**Confirmed on your phone recently (no need to test again):** the **+** button opens Files
(build 119), the **⋯** menu in a game opens (119), TV/AirPlay picture is sharper (119), turning to
landscape with no skin no longer freezes (120), the layout editor, the system pickers and a DS game.

---

# The list: what still needs testing

## A. Fixes from the last two builds (do these first)

**A1. Azahar (3DS) setting change no longer crashes**
1. Open a 3DS game and play for a few seconds.
2. Tap **⋯** → **Core settings...** and change a setting that says it needs a restart
   (for example **New 3DS / Old 3DS**).
3. Tap **Restart the game now**.
4. ✅ Good: the game starts again and the small text says the save "was saved with different core
   settings" and was not loaded. The game starts from the beginning instead.
5. ❌ Bad: the app closes. Tell me which setting you changed.
6. Bonus: change the setting back, restart again. Your old spot should load again.

**A2. Apple performance overlay switch (new in build 121)**
1. Open **Settings** → scroll to **DIAGNOSTICS** → the top switch, **Apple performance overlay**.
2. Leave it **off**, then open a game.
3. ✅ Good: Apple's grey box of FPS / GPU numbers is gone.
4. If it is still there: swipe Continuum away in the app switcher, open it again, and check again.
5. Turn it **on**, open a game: the box should come back (it may also need the app reopened).
6. Tell me if it never goes away. That would mean it is a different Apple overlay (for example the
   Game Mode one) and needs a different fix.

## B. Getting games in

**B1. Wi-Fi transfer** — Library → hold your finger on **+** → **Other sources (Wi-Fi, clipboard,
servers)** → Wi-Fi transfer. Type the address it shows into a computer browser on the same Wi-Fi,
upload a game. ✅ The game appears in the library.

**B2. Paste** — In the Files app, copy a game. In Continuum, hold **+** → Other sources → **Paste
from the clipboard**. ✅ The game appears.

**B3. Open in** — In the Files app, share a `.gba` file and pick Continuum. ✅ It is imported.

**B4. Zip / 7z** — Import a game inside a `.zip`. ✅ It shows up as the game, not as a zip.

**B5. Unknown disc** — Import a disc file the app cannot identify. ✅ It asks which system once, and
remembers your answer.

**B6. Save files from other emulators** — Open a game's card → **Save slots, export and import** →
import a save from another emulator (DS `.dsv` or PS1 `.mcr`). Start the game. ✅ The save is there.

**B7. WebDAV server (only if you have a NAS/computer sharing files)** — Hold **+** → Other sources →
add a server, browse it, import a game. (SMB is not in this build.)

## C. New systems — import one game each, tap it, see if it plays

For each one: ✅ cover shows, its own controls show, the game moves. ❌ Send the small text line.

| # | System | File type | Needs anything extra? |
| --- | --- | --- | --- |
| C1 | Game Boy (original) | `.gb` | No |
| C2 | Master System | `.sms` | No |
| C3 | SG-1000 | `.sg` | No |
| C4 | TurboGrafx-16 | `.pce` | No |
| C5 | Atari 2600 | `.a26` (rename `.bin` to `.a26`) | No |
| C6 | Famicom Disk System | `.fds` | `disksys.rom` in the Continuum folder. Getting a message naming that file counts as a pass |
| C7 | WonderSwan | `.ws` / `.wsc` | No |
| C8 | Neo Geo Pocket | `.ngp` / `.ngc` | No |
| C9 | Lynx | `.lnx` | Maybe `lynxboot.img` — the app names it if so |
| C10 | Atari 7800 / 5200 | `.a78` / `.a52` | 5200 may name a BIOS file |
| C11 | Virtual Boy | `.vb` | No |
| C12 | Pokemon Mini | `.min` | No |
| C13 | 32X / SuperGrafx | `.32x` / `.sgx` | No |
| C14 | Jaguar | `.j64` | No. Tell me how fast it feels |
| C15 | Saturn, Sega CD, PC Engine CD | `.cue` + all `.bin` together | A BIOS. The app names the exact file |
| C16 | Arcade | game zip like `mslug.zip` | Neo Geo games need `neogeo.zip` |
| C17 | C64, Amiga, DOS | DOS as a `.zip` | Amiga may need a Kickstart file. The **Keyboard** button opens a keyboard |
| C18 | DOOM | `.wad` | No |
| C19 | Dreamcast | `.gdi` or `.chd` | Maybe BIOS files. Tell me the speed |
| C20 | PSP | `.cso` | No. Expect it to be slowish |
| C21 | Nintendo 64 | `.z64` / `.n64` / `.v64` | No. Already ran Smash; just check it still does |
| C22 | Flash | `.swf` | No. Pad acts as arrows and Space |
| C23 | J2ME (old phone games) | `.jar` | No. Leave and come back: the save should still be there |

## D. Saves

**D1. Battery saves** — Save inside a game (Pokemon, Zelda), go back to the library, open it again.
✅ Your in-game save is still there.

**D2. Save slots** — In a game: **⋯** → **Save slots...**. Save into two slots, rename one,
overwrite one (it should ask first), load the older one, delete one. ✅ All work and show pictures.

**D3. Export and import a state** — Same screen: export a slot, import it back. ✅ It loads.

**D4. Quick save** — **⋯** → **Save to the next free slot**. ✅ A new slot appears in Save slots.

## E. Skins and controls

**E1. Skin holes** — Import a `.deltaskin` (re-import any old one). ✅ The game picture sits inside
the skin's screen area, both upright and sideways. On DS/3DS skins both screens are in their holes.
A circle pad works as a stick.

**E2. Manic skins** — Import a `.manicskin`. ✅ Buttons work, press animations show, switches slide,
button sounds play (unless the phone is on silent).

**E3. Skin per game** — Game card → pick a skin. In a game: **⋯** → **Skin...** to switch live.

**E4. Skin buttons that do things** — On a skin or an extra button, try quick save, fast forward,
filters, palette, screenshot, hide controls and quit.

**E5. Extra buttons** — **Settings** → **Move the on-screen controls** → add a button, a turbo
button and an action. ✅ They work in a game.

**E6. Edit a skin** — With a skin imported, use Edit this skin, move a button. ✅ The change shows in
a game.

**E7. Haptics and rumble** — **Settings** → **HAPTICS AND TURBO**. ✅ You feel button taps; a game
with vibration buzzes.

**E8. Controller auto-hide** — Connect a controller. **Settings** → **Hide the on-screen pad while a
controller is connected**. ✅ The pad disappears and the picture gets bigger.

**E9. Controllers and remapping** — **⋯** → **Controllers and button mapping...**: map one button to
another, make a second profile, switch to it. ✅ The new mapping works.

**E10. Keyboard, tilt, shake** — Computers: **⋯** → **Keyboard**. GBA tilt games (Yoshi
Topsy-Turvy): tilt the phone. Pokemon Mini: **⋯** → **Shake**.

## F. In-game menu (**⋯**)

**F1. Core settings** — **⋯** → **Core settings...** on a few systems (not just 3DS). Change one;
if it says restart, tap **Restart the game now**. ✅ No crash, and the change shows.

**F2. Filters** — **⋯** → **Filters...**: try CRT, LCD grid, dot matrix.

**F3. Speed** — **⋯** → **Speed**: 2x, 3x, 4x and slow motion. Sound follows.

**F4. Palette, rotate, discs** — Game Boy: **Next palette**. Any game: **Rotate picture**.
Multi-disc PS1 (`.m3u`): **Next disc**.

**F5. Cover from the game** — **⋯** → **Use this frame as the cover**. ✅ The game's cover changes.

**F6. DS and 3DS extras** — DS: **Close or open the lid**, **Blow into the microphone**. 3DS:
**HOME button**. PlayStation: **Analog pad**.

**F7. Cheats** — **⋯** → **Cheats and RAM search...**. Import a RetroArch `.cht`, or do a RAM
search (start, lose a life, filter "less", repeat, make cheat). ✅ The cheat holds.

## G. Two screens, TV and mouse

**G1. Swap screens** — In a DS/3DS game tap **Swap screens**. Also try the six layouts in
**Settings** → **TWO SCREENS, TV AND MOUSE**. ✅ Touch still lands where you tap.

**G2. DS touch** — Tap and drag on the lower screen. ✅ The game reacts exactly where you touch.

**G3. TV** — AirPlay or a cable during a game. ✅ Game on the TV, controls on the phone. Then **⋯**
→ **TV scaling** and **TV layout**.

**G4. Mouse** — Turn on mouse for SNES in Settings, play Mario Paint. ✅ Drag moves, tap clicks.

**G5. 3DS microphone** — Settings → allow microphone, play a 3DS game that listens.

## H. Online and accounts

**H1. Online play** — Two phones, same Wi-Fi, same game. **⋯** → **Play online with a second
phone**. Host on one, join from Nearby on the other. ✅ Both say connected.

**H2. Achievements** — **Settings** → **RETROACHIEVEMENTS**, log in, play an NES, SNES or GBA game
with achievements. ✅ A banner pops up on unlock.

**H3. Cloud sync** — **Settings** → **CLOUD SYNC**, pick an iCloud Drive folder. Save in a game, go
back to the library. ✅ The status says files were sent.

## Known not working or not built (no need to test)

- SMB (NAS shares) is out of this build. WebDAV works.
- GameCube and Wii: not possible without JIT.
- Camera and Amiibo do not reach 3DS games (the 3DS core cannot take them).
- `.cht` RAM cheats on GBA may read the wrong memory.
- Online play over the internet needs port 55435 opened on the host's router.
- JIT is left out on purpose.

---

# Already confirmed

## What is already confirmed working

This app has been run on an iPhone 17 Pro Max and it plays games. The first cores each ran a real
game at 60 fps with 0 dropped frames. The app now carries **32 cores**, so the `cores:` line should
read about **32 of 32 declared**. DS (Mario Kart DS, SoulSilver), N64 (Smash) and 3DS (Mario Kart)
have also run since:

| System | Game that ran | Core | Frames counted in the screenshot |
| --- | --- | --- | --- |
| NES | Kart Fighter | fceumm | 285 |
| SNES | Super Mario World (U) | snes9x | 466 |
| Game Boy Advance | Pokemon Emerald (USA, Europe) | mgba | 321 |
| Game Boy Color | Pokemon Yellow (UE) | mgba | 1162 |
| Genesis / Mega Drive | Mortal Kombat 3 (USA) | genesis_plus_gx | 2354 |
| Game Gear | Simpsons: Krusty's Fun House (U) | genesis_plus_gx | 2188 |
| PlayStation 1 | Crash Bandicoot (USA) | pcsx_rearmed | 2390 |
| PlayStation 1, Beetle | Crash Bandicoot | Beetle PSX HW | about 2900, with `scph1001.bin`, build 98 |

Alongside those: five games imported in one go (`imported 5 of 5`), a PlayStation `.cue` and its
`.bin` imported together and booted with no BIOS on the original core, the Crash Bandicoot row showing the
size of the whole game as `CUE · 602.8 MB · pcsx_rearmed`, and the Library reading
`library: 6 game(s) of 7 file(s) in Documents`, which is correct: the seventh file is that `.bin`
track, and it is deliberately not a row you can tap.

Everything below has since been confirmed on a device as well, in the order it was built:

| Confirmed | Notes from the person testing it |
| --- | --- |
| Sound | Tried on four games |
| Fast forward | Works, and sound stays clean while it runs |
| Volume and mute | Works, including during fast forward |
| Screen fit: Fit, Pixel perfect, Fill | "Changes how it sits" |
| Scaling: Sharp, Smooth | Works; smooth is blurrier, which is what it is |
| Rewind | Works |
| Rewind Off and On | Works |
| Save states | Work, and deleting one works |
| Auto-save and resume | Works |
| Cheats | Work |
| Bluetooth controller | Works |
| Controller and thumbs together | Works, which was the hard part of the input rewrite |
| Library layout, Grid and List | Works |

Two of those lines are narrower than they sound. Cheats means a code you type, not a search and not a file import. Fast forward is about 4x, not 5x. The 50-slot save manager is in the list above (D2).

So this checklist is no longer asking whether any of it works. **It is a regression check.** Each
test below says what already passed, and if one of those fails on a new build then something that
used to work has broken, which is worth telling me straight away.

Before you start, read [README.md](README.md) if you have not.

## The single most useful thing you can do

The app has a block of small text at the top of the screen. That text is the only diagnostic
there is. An app installed outside the App Store has no debugger attached, so there is no log,
no crash report I can read, and no way for me to watch what happened. That text block is it.

So when something does not work:

- **"It did nothing" tells me almost nothing.** It rules out one thing out of about fifteen.
- **The exact text on screen usually tells me the answer immediately.** Every line is worded to
  rule a different cause in or out.

A screenshot of the whole screen is perfect. If you are typing it out instead, the **last line
of status text** is the one that matters most, because it is the most recent thing the app
tried to do.

## Test 1: does the app install and open

**Already passed.** It used to report 5 of 5 declared; it should now say 32 of 32.

**Do this**

1. Install `Continuum.ipa` on the phone.
2. Open it.
3. Take a screenshot of whatever appears.

**You should see**

- A black screen.
- A block of small text at the top. Its first line names the build. Under that there is a status
  line, a line starting `cores:`, a line starting `BIOS`, a line about the graphics device, and a
  line counting frames and fps.
- The status line should say something close to `surface ready - tap + to add a game`.
- The `cores:` line should say **32 of 32 declared**.
- The **Library**, with a **+** button that opens Files.

**Tell me if it did not work**

- Whether it failed to install, or installed but would not open, or opened and closed again.
- **Which installer you used.** This matters more than it sounds like it should, because
  different installers keep or strip the app's special permissions, and that changes what is
  likely wrong.
- If it opened: the screenshot.
- If the `cores:` line says fewer than 32, send that whole line word for word. It names the
  missing piece, and that means the build is at fault, not your phone.
- If there is no line about the graphics device, say so. That is a specific failure and it is
  useful to know.

## Test 2: does a cartridge game import and play

**Already passed**, on `.nes` with Kart Fighter and on `.gba` with Pokemon Emerald. Both played
at 60 fps with 0 dropped frames.

**Start with a `.nes` or a `.gba` file.** One file, nothing else needed, no extra files and no
BIOS. That keeps the question simple: does emulation work at all? If you start with a
PlayStation game instead and it fails, we will not know whether emulation is broken or whether
it was just the multi-file part.

**Do this**

1. Put a `.nes` or `.gba` file somewhere your phone can reach it, such as Files or iCloud Drive.
2. Open Continuum and tap **+** (it opens Files).
3. Pick the file and tap **Open**.
4. The game should now appear in the Library list.
5. Tap it.
6. Wait a few seconds, then take a screenshot.

**You should see**

- After importing: a row in the Library with the filename, and under it a detail line ending
  with the name of the core it will use. A `.nes` file should say `fceumm`. A `.gba` file should
  say `mgba`.
- After tapping: the status line changes to `running:` followed by the filename and the core.
- **A picture from the game filling the black area behind the text.**
- The bottom line of the text block counting upward: frames rising, fps somewhere near 60, and
  dropped staying at or near 0.

**Tell me if it did not work**

Say which of these it was, because each one points somewhere different:

- The import picker never opened.
- The picker opened but the game did not appear in the list afterwards.
- It appeared in the list, but the detail line named the wrong core, or no core.
- You tapped it and nothing happened at all.
- You tapped it and the status line changed to an error. **Send that line word for word.**
- The frame counter stayed at 0.
- The frame counter went up but the screen stayed black. This one is important and specific, so
  say it plainly if that is what happened: it means the emulator ran and the picture did not
  arrive.
- Sound but no picture, or picture but no sound.

A screenshot covers nearly all of this at once.

## Test 3: the other cartridge systems

**Mostly passed. Two rows are still untried, and they are the only gap left in this checklist.**
Each is the same routine as Test 2, with a different file.

| File to try | Console | Core it should name | Where it stands |
| --- | --- | --- | --- |
| `.sfc` or `.smc` | SNES | snes9x | Passed on `.smc` with Super Mario World |
| `.gb` or `.gbc` | Game Boy, Game Boy Color | mgba | Passed on `.gbc` with Pokemon Yellow. `.gb` not tried |
| `.sms` | Master System | genesis_plus_gx | **Not tried yet.** Still worth doing |
| `.md` or `.gen` | Genesis / Mega Drive | genesis_plus_gx | Passed on `.md` with Mortal Kombat 3 |
| `.gg` | Game Gear | genesis_plus_gx | Passed on `.gg` with Krusty's Fun House |

The two untried rows are the ones to spend a test on. Neither is a worry: `.sms` runs on the same
core as the `.md` and `.gg` games that played, and `.gb` runs on the same core as the `.gba` and
`.gbc` games that played. They just have not been seen.

**You should see** the same as Test 2 for each one: the right core named in the list, a picture,
and the frame counter climbing.

**Tell me if it did not work**

- Which extensions worked and which did not. A list is fine, for example "nes and gba fine, sms
  black screen, gg not tried".
- For each failure, the status line.
- One case to watch for on Master System, Genesis and Game Gear: all three use the same core, so
  if one of them plays and another shows a scrambled or wrong-looking picture, that is a
  different problem from a black screen. Say which it was.

## Test 4: does a PlayStation game work

**Already passed.** Crash Bandicoot, imported as a `.cue` plus its `.bin` in one go, played with
no BIOS file on the original core. A later run also booted Crash on Beetle with `scph1001.bin`
(build 98, about 60 fps, about 2900 frames). That BIOS boot happened. The no-BIOS result is only
the older option, and those frames are not proof the GPU handoff is done.

It has an extra way to fail that has nothing to do with emulation, so on a fresh build it is
still worth leaving until the cartridge systems work.

**Do this**

1. Find a PlayStation game made of a `.cue` file plus one or more `.bin` files.
2. Tap **+**.
3. In the picker tap **Select**, then tap the `.cue` **and every `.bin`**, then tap **Open**.
   They must go in together, in one import.
4. Tap the `.cue` in the Library.

**You should see**

- The `.cue` in the Library, with a detail line naming `pcsx_rearmed`. The size on that line is
  the whole game, the sheet plus the tracks it names, not the few bytes of the `.cue` itself.
  Crash Bandicoot reads `CUE · 602.8 MB · pcsx_rearmed`.
- The `.bin` files **not** in the list. That is correct and not a bug. They are on disk beside
  the `.cue`, and the emulator reads them itself. Only the `.cue` is something you tap.
- The status line going to `running:` and then a picture.
- A line starting `BIOS (pcsx_rearmed):`. It will probably say `none, HLE fallback`. That is
  fine. It means the emulator is filling in for the missing console startup software itself.
  Some games are fussier than others about that.

**Tell me if it did not work**

- How many files you selected, and whether they went in as one import or several.
- Whether the `.bin` files ended up in the list. They should not be.
- The status line, word for word.
- The `BIOS (pcsx_rearmed):` line as it appears.
- If it got as far as a picture and then froze, say roughly how long it ran for. A game that
  runs for two seconds and stops is a different fault from one that never starts.

## Test 5: the Files app route

**Already passed.** Files dropped into the folder showed up in the Library.

This is an alternative to the import button, and worth confirming separately because it uses a
different path through the app.

**Do this**

1. Open the iPhone **Files** app.
2. Go to **On My iPhone** and find the **Continuum** folder.
3. Copy a game file into it.
4. Open Continuum, or leave and come back to it.

**You should see** the game in the Library list, exactly as if you had imported it.

**Tell me if it did not work**

- Whether the Continuum folder is visible in Files at all. If it is not, that is its own
  problem and worth knowing.
- Whether the file is in the folder but not in the list.

## Reading the diagnostic text

Rough guide to the lines, top to bottom:

| Line | What it is |
| --- | --- |
| First line | Names the build. Confirms you are running what you think you are running. |
| Status line | The most recent thing the app did or tried to do. **This is the line to report.** |
| `cores:` | How many of the emulator cores are actually inside the app. Should be 32 of 32. |
| `BIOS (...)` | Only relevant to PlayStation. `none, HLE fallback` is normal. |
| `library:` | How many games the app found, and how many files that came from. A PlayStation `.bin` track counts as a file and not as a game, so `6 game(s) of 7 file(s)` is right for six games where one of them is a `.cue` with one track. |
| Graphics line | Describes the graphics device. If this line is missing, drawing never started. |
| `... frames · ... fps · ... dropped` | Whether the emulator is running. 0 frames means it never ran. Frames climbing with a black screen means it ran but the picture did not arrive. |

Two combinations worth recognising, because they send me to completely different places:

- **Frames at 0**: the emulator never started. The problem is loading, not drawing.
- **Frames climbing, screen still black**: the emulator is running fine and the picture is
  getting lost on the way to the screen.

## If you only send me one thing

Send a screenshot of the whole screen at the moment it went wrong, and say which test number you
were on. That is enough to start from almost every time.
