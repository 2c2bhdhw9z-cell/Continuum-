# What to test, and what to tell me

Last updated 7 October 2026, for **build 141 or newer**. The newest install is always on the
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
On 5 October with build 122: the eye button that hides the top bar (and that it remembers), a
paused game staying paused after leaving the app, a 3DS skin sideways with nothing on the picture,
importing several skins at once, the tidier small text, changing a 3DS "restart" setting without a
crash, fast forward at 2x, the CRT filter, taking a cover from the game, and Save to the next free
slot. On 5 October with build 125: save slot pictures, save and load on 14 systems, exporting and
importing a state file, the Apple overlay switch, restarting from Core settings, cover lookups off,
the RetroAchievements login button, paste, Open in, zip files, and games on Game Boy,
TurboGrafx-16, Atari 2600, Jaguar and Pokemon Mini. Also on build 125: the sharper cover from the
game, battery saves, renaming, saving over and deleting slots, export and import on a non-3DS game,
the keyboard, tilt and shake, 3x, 4x and slow motion, a RetroAchievements unlock banner, haptics
and rumble, hiding the pad with a controller, the LCD grid and dot matrix filters, palette and
rotate, swap screens and the six layouts, and the game on a TV. On build 126: 3DS saves load, an
imported save keeps its picture, the small text and (i) with a skin, and typing a cheat code inside
a game.

---

# The list: what still needs testing

## A. Not in a build yet — deleting the app stops losing everything

Needs a build, which is waiting on your word. **Set the sync folder up BEFORE you next delete the
app**, or there is nothing in the cloud to come back from.

**M1. Set it up once**
1. **Settings** → **CLOUD SYNC** → **Choose a sync folder**. Pick anywhere in Files — iCloud Drive,
   Google Drive, Dropbox. It makes a "Continuum Sync" folder inside whatever you pick.
2. If you want your games backed up too, turn on **Back up the games too**. Leave it off if you are
   short on cloud space — it can be tens of gigabytes.
3. Tap **Sync now** and wait for the line above the buttons to say it finished.

**M2. The real test: delete it and put it back**
1. Star a couple of games, import a skin, then **Sync now**.
2. Delete Continuum the way you normally do, install the new build, and choose the same folder.
3. Tap **Sync now**, then **close Continuum fully and open it again** (skins and settings only load
   at launch).
4. ✅ Good: your save states, battery saves, cheats, settings, cover choices, manuals, **starred
   games** and **imported skins** are all back. With the games switch on, the games are back too.
5. ❌ Tell me anything that did NOT come back.

**M3. BIOS files are not copied**
1. If you have BIOS files in the Continuum folder, look in your cloud folder's "Continuum Sync"
   after a sync with the games switch on.
2. ✅ Good: your games are there, and no BIOS file is. Disc games should have their `.bin`/track
   files beside the `.cue`, or they would not load when restored.

## A. New in build 141 (do these first)

**L1. All your games show in Recently added**
1. Import a big batch of games at once (the more the better).
2. ✅ Good: **Recently added** on Home shows **all of them**, not 18. The count beside the title
   should match what you actually imported.
3. ✅ Good: the newest ones are at the FRONT of the shelf, in the order they imported — not in
   alphabetical order, and not buried behind games you added days ago.
4. ✅ The big featured game at the top should be one you just added.

**L2. Scrolling**
1. On Home, scroll up and down through the shelves a few times, then scroll a long shelf sideways.
2. ✅ Good: it should feel smoother than build 138, and should NOT get worse the longer you scroll
   or the more games you have. Scrolling back to covers you already passed should be instant.
3. Tell me if it is better, the same, or worse — "the same" is a useful answer here.

**L3. The artwork should look BETTER, not worse**
1. Look at the covers on Home, the big featured one especially, and open a game's card.
2. ✅ Good: covers look as sharp or sharper than before, particularly the big featured one.
3. ❌ If anything looks blurrier or softer than build 138, tell me straight away — nothing in this
   build shrinks artwork, so that would mean I got something wrong.

## A (from build 140)

**J3. The JIT line says what to do about it now**
1. **Settings** → **TECHNICAL DETAILS** → the **JIT** row.
2. ✅ Good: it says this copy wasn't signed for JIT, and then tells you that signing it again with
   the **get-task-allow** file that comes with Continuum would give you the option.
3. ✅ The **Use JIT when it's available** line below should now read "Makes PSP, 3DS, PlayStation,
   N64 and Dreamcast faster" — with **no** "on older iPhones" on the end. The N64 stopped needing
   that in build 138.

**If you want to actually test JIT** (optional, and the only way anyone can — nothing in the JIT
work has ever run on a phone): re-sign the .ipa with the `get-task-allow.entitlements` file that
sits beside it, using a signing app that asks you for an entitlements file, then attach StikDebug.
Your current copy was signed without it, which is why JIT can never switch on for that install.

## A (from build 133) — J1 PASSED on build 138

**J1. The JIT line** — ✅ **Passed, 7 October, build 138.** The owner's screenshot shows
`Off. The way this copy was signed doesn't allow JIT. That's fine, everything still works, just
slower on the heavy systems.`, the **Use JIT when it's available** switch on, and no
"Turn on JIT with StikDebug" button — which is correct for a copy signed without `get-task-allow`.
That sentence was reworded in build 140 because it named a cause with no action (J3 above). (your phone can't use JIT, so this only checks it says so nicely)
1. **Settings** → scroll to **TECHNICAL DETAILS** at the bottom.
2. ✅ Good: the first row, **JIT**, starts with **Off.** and says why in one plain sentence. Tell
   me the sentence. Under it is a switch, **Use JIT when it's available**: leave it on.
3. ✅ Good: there is no **Turn on JIT with StikDebug** button, because this copy can't use JIT.
   If you DO see one, tell me.

**J2. Nothing got slower**
1. Play Mario Kart 7 for a minute, then a PlayStation game, then a PSP game if you have one, then
   a Dreamcast game if you have one.
2. ✅ Good: each runs the same as it did before. All four of those emulators changed inside for
   JIT, so this is checking that the no-JIT way they actually run on your phone still works.
3. Tap **(i)** while a game is running. ✅ A line starting **JIT:** says it is off and names your
   iPhone and iOS version. Send me a screenshot of that line.

## A (from build 129)

**A1. A cleaner player**
1. Open any game.
2. ✅ Good: no fps line or small text sits on the game. After a save or other action, a short
   message shows at the top for a few seconds, then fades.
3. Tap **(i)** in the top bar. ✅ The fps line and the full technical block show. Tap again to hide.

**A2. A cleaner Settings**
1. Open **Settings**.
2. ✅ Good: **FEEDBACK** is the first card, **TECHNICAL DETAILS** and **ABOUT** (with the version)
   are the last two. The old developer notes about JIT and "step 4" are gone.

**A3. Feedback goes to your email**
1. **Settings** → **Send feedback**, type anything → **Send**.
2. ✅ Good: Mail opens addressed to idkplswrk@gmail.com. Send it and check that inbox.

**A4. The 3DS model change** (from build 127)
1. In Mario Kart 7: **⋯** → **Core settings...** → **System Model** → **Original 3DS**.
2. ✅ Good: the game keeps running and **Restart the game now** appears. Tap it: no crash.
3. Set it back to **New 3DS** and restart again.

**A5. Cheats switch off** (from build 127)
1. In the Pokemon game: **⋯** → **Cheats and RAM search...**. A line at the top names the game and
   version. Tell me what it says.
2. Turn both walk-through-walls cheats off. ✅ Good: the game no longer freezes when you move.

## A (older). Things to notice in passing

**A7. Things to notice in passing** (no need to set up)
- The small text in the player should always read as plain English. If you ever see something like
  `SaveState(reason: "...")` in it, send a screenshot.
- When the app asks **Which system?** for a file, GameCube, Wii and Symbian are no longer offered
  (none of them can run in this build).
- PlayStation with **Beetle PSX HW** selected: a BIOS file whose name is in capitals (for example
  `SCPH1001.BIN`) is now found, and the BIOS line in the small text starts
  `BIOS (mednafen_psx_hw)`, which is Beetle, not the other PlayStation core.
- 3DS (Mario Kart 7): after a race ends or the screen changes, does the game stutter or stay
  smooth? Tell me either way.
- Jaguar: how fast did the game feel? Full speed, a bit slow, or very slow?

## B. Getting games in

**B5. Unknown disc** — Import a disc file the app cannot identify. ✅ It asks which system once, and
remembers your answer.

**B6. Save files from other emulators** (only if you have one) — Open a game's card → **Save slots,
export and import** → import a save from another emulator (DS `.dsv` or PS1 `.mcr`). Start the
game. ✅ The save is there.

## C. New systems — import one game each, tap it, see if it plays

For each one: ✅ cover shows, its own controls show, the game moves. ❌ Send the small text line.

| # | System | File type | Needs anything extra? |
| --- | --- | --- | --- |
| C2 | Master System | `.sms` | No |
| C3 | SG-1000 | `.sg` | No |
| C6 | Famicom Disk System | `.fds` | `disksys.rom` in the Continuum folder. Getting a message naming that file counts as a pass |
| C7 | WonderSwan | `.ws` / `.wsc` | No |
| C8 | Neo Geo Pocket | `.ngp` / `.ngc` | No |
| C9 | Lynx | `.lnx` | Maybe `lynxboot.img` — the app names it if so |
| C10 | Atari 7800 / 5200 | `.a78` / `.a52` | 5200 may name a BIOS file |
| C11 | Virtual Boy | `.vb` | No |
| C13 | 32X / SuperGrafx | `.32x` / `.sgx` | No |
| C15 | Saturn, Sega CD, PC Engine CD | `.cue` + all `.bin` together | A BIOS. The app names the exact file |
| C16 | Arcade | game zip like `mslug.zip` | Neo Geo games need `neogeo.zip` |
| C17 | C64, Amiga, DOS | DOS as a `.zip` | Amiga may need a Kickstart file. The **Keyboard** button opens a keyboard |
| C18 | DOOM | `.wad` | No |
| C19 | Dreamcast | `.gdi` or `.chd` | Maybe BIOS files. Tell me the speed |
| C20 | PSP | `.cso`, or a PSP `.iso` / `.chd` / `EBOOT.PBP` (it should open as PSP; if it asks, pick PlayStation Portable) | No. Expect it to be slowish |
| C22 | Flash | `.swf` | No. Pad acts as arrows and Space |
| C23 | J2ME (old phone games) | `.jar` | No. Leave and come back: the save should still be there |

## E. Skins and controls

**E1. Skin holes** — A 3DS skin held sideways is already confirmed. Still to check: hold the phone
upright with a skin, and try a DS skin. ✅ The game picture sits inside the skin's screen area, and
on a DS skin both screens are in their own holes. A circle pad works as a stick. If a DS or 3DS
skin shows BOTH screens squashed into the top screen area and nothing in the bottom one, that skin
was imported before build 109: delete it in the skin library and import the same skin file again.
(Older builds only kept the first screen area of a skin, and the app cannot read the original file
again by itself.)

**E2. Manic skins** — Import a `.manicskin`. ✅ Buttons work, press animations show, switches slide,
button sounds play (unless the phone is on silent).

**E3. Skin per game** — Game card → pick a skin. In a game: **⋯** → **Skin...** to switch live.

**E4. Skin buttons that do things** — On a skin or an extra button, try quick save, fast forward,
filters, palette, screenshot, hide controls and quit.

**E5. Extra buttons** — **Settings** → **Move the on-screen controls** → add a button, a turbo
button and an action. ✅ They work in a game.

**E6. Edit a skin** — With a skin imported, use Edit this skin, move a button. ✅ The change shows in
a game.

**E9. Controllers and remapping** — **⋯** → **Controllers and button mapping...**: map one button to
another, make a second profile, switch to it. ✅ The new mapping works.

## F. In-game menu (**⋯**)

**F1. Core settings** — **⋯** → **Core settings...** on a few systems (not just 3DS). Change one;
if it says restart, tap **Restart the game now**. ✅ No crash, and the change shows.

**F4. Next disc** (only if you have a multi-disc PS1 game, as an `.m3u`) — **⋯** → **Next disc**.
✅ The game sees the next disc. (Palette and rotate are confirmed.)

**F6. DS and 3DS extras** — DS: **⋯** → **Close or open the lid**. DS blowing is NOT in the ⋯
menu: it is a round microphone button at the top left of the DS game screen, which you hold down
while the game asks you to blow. 3DS: **⋯** → **HOME button**. PlayStation: **⋯** → **Analog pad**.

**F7. Cheats** — **⋯** → **Cheats and RAM search...**. Import a RetroArch `.cht`, or do a RAM
search (start, lose a life, filter "less", repeat, make cheat). ✅ The cheat holds. New in 122: a
RetroArch `.cht` file for a **Game Boy Advance** game should now work too (it used to change the
wrong part of memory). Worth one try with a GBA cheat file.

## G. Two screens, TV and mouse

**G2. DS touch** — Tap and drag on the lower screen. ✅ The game reacts exactly where you touch.

**G4. Mouse** — Turn on mouse for SNES in Settings, play Mario Paint. ✅ Drag moves, tap clicks.

**G5. 3DS microphone** — Settings → allow microphone, play a 3DS game that listens.

## H. Accounts and cloud

**H3. Cloud sync** — **Settings** → **CLOUD SYNC** → **Choose a sync folder**, and pick a folder in
iCloud Drive. Save in a game, go back to the library. ✅ The status says files were sent. New in
122: Flash and J2ME saves, PDF manuals, Amiibo files, your "which system is this" answers and saved
servers are synced too.

## Needs a computer or a second phone (skip unless you have one)

**B1. Wi-Fi transfer** (a computer) — Library → hold your finger on **+** → **Other sources (Wi-Fi,
clipboard, servers)** → Wi-Fi transfer. Type the WHOLE address it shows into a computer browser on
the same Wi-Fi, including the short code at the end (for example `http://192.168.1.20:8080/k7m2qx/`),
and upload a game. ✅ The game appears in the library. ✅ The address without the code shows nothing.

**B7. WebDAV server** (a NAS or computer sharing files) — Hold **+** → Other sources → add a server,
browse it, import a game. (SMB is not in this build.)

**H1. Online play** (a second phone) — Two phones, same Wi-Fi, same game. **⋯** → **Play online with
a second phone**. Host on one, join from Nearby on the other. ✅ Both say connected. Then on the
second phone, leave online play and join again from Nearby. ✅ The first phone says the other phone
left and is waiting, keeps its game going, and lets the second phone back in.

## Known not working or not built (no need to test)

- Typed cheat codes do nothing on the 3DS, Dreamcast, Atari 2600, arcade, Saturn (Yabause),
  Amiga, C64, Lynx, Atari 5200, Virtual Boy, PC Engine CD and Pokemon Mini: those emulators ignore
  them. The RAM search can still make a cheat where the game's memory can be read.

- SMB (NAS shares) is out of this build. WebDAV works.
- GameCube and Wii: not possible without JIT.
- Camera and Amiibo do not reach 3DS games (the 3DS core cannot take them).
- Online play over the internet needs port 55435 opened on the host's router.
- A WebDAV server that arrives on a second phone through cloud sync has no password there (passwords
  never leave the phone they were typed on). Remove it and add it again on that phone.
- JIT is left out on purpose.

---

# Already confirmed

## What is already confirmed working

This app has been run on an iPhone 17 Pro Max and it plays games. The first cores each ran a real
game at 60 fps with 0 dropped frames. The app now carries **32 cores**, so the `cores:` line should
read **32 of 32 declared**. The first cores' results (DS, N64 and 3DS are in the second table):

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
| Nintendo DS (build 80) | Mario Kart DS and Pokémon SoulSilver, both screens, about 60 fps, 0 dropped |
| Nintendo 64 (build 97) | Smash reached the character select, about 60 fps, no JIT |
| Nintendo 3DS | Mario Kart runs (builds 100 and 101, with a stutter after transitions); Mario Kart 7 on build 122 |
| **+** opens Files (119) | Works |
| The **⋯** menu in a game (119) | Opens |
| Sharper TV/AirPlay picture (119) | Sharper |
| Landscape with no skin (120) | No longer freezes |
| The eye button that hides the top bar (122) | Hides and shows it, and remembers |
| Pause stays paused (122) | A paused game stays paused after leaving the app |
| 3DS skin sideways, both screens (122) | Mario Kart 7 screenshot: both screens in their holes, nothing on the picture |
| Several skins at once (122) | Imported together |
| Tidier small text (122) | The diagnostic text reads tidier |
| 3DS restart setting (122) | Changed without a crash |
| Fast forward 2x (122) | Works |
| CRT filter (122) | Works |
| Cover from the game (122) | Works |
| Save to the next free slot (122) | The slot appears |
| Save slot pictures (125) | A Mario Kart 7 slot shows the game, tall, both screens |
| Save and load (125) | One game each on NES, SNES, Game Boy, GBC, GBA, Game Gear, Mega Drive, PS1, DS, TurboGrafx-16, Atari 2600, N64, Jaguar and Pokemon Mini |
| New systems (125) | Game Boy, TurboGrafx-16, Atari 2600, Jaguar and Pokemon Mini each ran a game |
| Export and import a state file (125) | A 3DS slot went out to Files and came back as a new slot (loading it is the known 3DS bug) |
| Apple performance overlay switch (125) | Works |
| Restart from Core settings (125) | Comes back inside the skin; with auto-save off it starts from the beginning |
| Cover lookups off (125) | Works |
| RetroAchievements login button (125) | Says Logging in..., and a second tap does nothing |
| Paste, Open in, zip (125) | Each one brought the game in |
| Sharper cover from the game (125) | Works |
| Battery saves (125) | An in-game save is still there after leaving and coming back |
| Save slots: rename, save over, delete (125) | All work |
| Export and import on a non-3DS game (125) | The imported slot loads (it has no picture yet) |
| Keyboard, tilt, shake (125) | Work |
| Speeds 3x, 4x and slow motion (125) | Work |
| RetroAchievements (125) | The unlock banner pops up |
| Haptics and rumble (125) | Taps felt, vibration works |
| Hide the pad while a controller is connected (125) | Works |
| Filters: LCD grid and dot matrix (125) | Work |
| Next palette and Rotate picture (125) | Work |
| Swap screens and the six layouts (125) | Work, and touch lands where you tap |
| Game on the TV, TV scaling and TV layout (125) | Work |
| 3DS saves load (126) | Save and load on Mario Kart 7, and the imported slot too |
| Imported save keeps its picture (126) | Shows straight away |
| Small text and (i) with a skin (126) | The message flashes; (i) opens the full block |
| Typing a cheat code in a game (126) | The box is there and takes the code |

Two of those lines are narrower than they sound. Cheats means a code you type, not a search and not a file import. Fast forward is about 4x, not 5x.

Tests 1 to 5 below are a regression check: each says what already passed, and if one fails on a
new build, something that used to work has broken, which is worth telling me straight away.

Before you start, read [README.md](README.md) if you have not.

## The single most useful thing you can do

The app has a block of small text: near the top of the player, and in the Library behind the thin
line above the tabs. That text is the only diagnostic there is. An app installed outside the App
Store has no debugger attached, so there is no log, no crash report I can read, and no way for me
to watch what happened. That text block is it.

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

1. Install the newest `Continuum-<number>.ipa`.
2. Open it.
3. Tap the thin line of small text just above the tabs at the bottom, so the diagnostic block
   opens, and take a screenshot.

**You should see**

- The **Library** (Home, All Games, Favorites and Settings along the bottom), with a **+** button
  that opens Files.
- A thin line of small text above the tabs. Tapping it opens the diagnostic block. Its first line
  names the build. Under that there is a status line, a line starting `cores:`, usually a line
  starting `BIOS`, a line about the graphics device, and a line counting frames and fps.
- The status line should say something close to `surface ready - tap + to add a game`.
- The `cores:` line should say **32 of 32 declared**.

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

**Passed on `.smc`, `.gbc`, `.md`, `.gg` and (build 125) `.gb`. `.sms` is test C2.** Each is the
same routine as Test 2, with a different file.

| File to try | Console | Core it should name | Where it stands |
| --- | --- | --- | --- |
| `.sfc` or `.smc` | SNES | snes9x | Passed on `.smc` with Super Mario World |
| `.gbc` | Game Boy Color | mgba | Passed on `.gbc` with Pokemon Yellow |
| `.md` or `.gen` | Genesis / Mega Drive | genesis_plus_gx | Passed on `.md` with Mortal Kombat 3 |
| `.gg` | Game Gear | genesis_plus_gx | Passed on `.gg` with Krusty's Fun House |

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
(build 98, about 60 fps, about 2900 frames). The no-BIOS result is only the older option. The
Beetle frames came from its software renderer; its hardware (Vulkan) renderer has not been seen on
a phone.

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
| `BIOS (<emulator>)` | The BIOS file for the running game's system. `none, HLE fallback` is normal on the standard PlayStation emulator; Beetle and some other systems need a real file, and the line names it. |
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
