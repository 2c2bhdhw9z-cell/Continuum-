# What to test, and what to tell me

This is a checklist you can work through on your phone. Do the tests in order. Each one only
makes sense if the one before it passed.

For every test there are three parts: what to do, what you should see if it worked, and what to
send me if it did not.

## What is already confirmed working

This build has been run on an iPhone 17 Pro Max and it plays games. All five cores ran a real
game, each at 60 fps with 0 dropped frames, with the `cores:` line reading 5 of 5 declared every
time:

| System | Game that ran | Core | Frames counted in the screenshot |
| --- | --- | --- | --- |
| NES | Kart Fighter | fceumm | 285 |
| SNES | Super Mario World (U) | snes9x | 466 |
| Game Boy Advance | Pokemon Emerald (USA, Europe) | mgba | 321 |
| Game Boy Color | Pokemon Yellow (UE) | mgba | 1162 |
| Genesis / Mega Drive | Mortal Kombat 3 (USA) | genesis_plus_gx | 2354 |
| Game Gear | Simpsons: Krusty's Fun House (U) | genesis_plus_gx | 2188 |
| PlayStation 1 | Crash Bandicoot (USA) | pcsx_rearmed | 2390 |

Alongside those: five games imported in one go (`imported 5 of 5`), a PlayStation `.cue` and its
`.bin` imported together and booted with no BIOS present, the Crash Bandicoot row showing the
size of the whole game as `CUE · 602.8 MB · pcsx_rearmed`, and the Library reading
`library: 6 game(s) of 7 file(s) in Documents`, which is correct: the seventh file is that `.bin`
track, and it is deliberately not a row you can tap.

So this checklist is no longer asking whether any of it works. **It is a regression check.** Each
test below says what already passed, and if one of those fails on a new build then something that
used to work has broken, which is worth telling me straight away. Two file types have still never
been tried, `.sms` and `.gb`, and they are marked as such in Test 3.

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

**Already passed.** The app installed, opened, and reported 5 of 5 cores declared.

**Do this**

1. Install `Continuum.ipa` on the phone.
2. Open it.
3. Take a screenshot of whatever appears.

**You should see**

- A black screen.
- A block of small text at the top. Its first line names the build. Under that there is a status
  line, a line starting `cores:`, a line starting `BIOS`, a line about the graphics device, and a
  line counting frames and fps.
- The status line should say something close to `surface ready - tap Import Games to add a game`.
- The `cores:` line should say **5 of 5 declared**.
- Below the text block there is a **Library** heading with an **Import Games** button.

**Tell me if it did not work**

- Whether it failed to install, or installed but would not open, or opened and closed again.
- **Which installer you used.** This matters more than it sounds like it should, because
  different installers keep or strip the app's special permissions, and that changes what is
  likely wrong.
- If it opened: the screenshot.
- If the `cores:` line says fewer than 5, send that whole line word for word. It names the
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
2. Open Continuum and tap **Import Games**.
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
no BIOS file present at all.

It has an extra way to fail that has nothing to do with emulation, so on a fresh build it is
still worth leaving until the cartridge systems work.

**Do this**

1. Find a PlayStation game made of a `.cue` file plus one or more `.bin` files.
2. Tap **Import Games**.
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
| `cores:` | How many of the five emulator cores are actually inside the app. Should be 5 of 5. |
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
