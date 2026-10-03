# What to test, and what to tell me

This file has two halves and the first one is the one to look at.

**[The queue](#the-queue-what-still-needs-testing)** is the short list of things that have been
built and never tried on a device. It is meant to be worked through and ticked off, and it is the
only part that changes often.

**[The regression checklist](#what-is-already-confirmed-working)** is everything that has already
been confirmed. It is not asking whether the app works any more; it is there so that if a new
build breaks something that used to work, there is a written record of what "used to work" meant.

---

# The queue: what still needs testing

Nothing in here has been confirmed on a device. Tick a line when it works, or tell me what it did
instead. **The most useful reply is the exact text from the diagnostics panel**, which is the ⓘ
button in the player; see [Reading the diagnostic text](#reading-the-diagnostic-text).

| | What to do | What it should do | Why it is in the queue |
| --- | --- | --- | --- |
| ☑ **1. Does the app open at all** | — | — | **PASSED.** Opens and stays open, signed with a DISTRIBUTION certificate in ESign with "Auto modify jailbreak dependencies" on. The launch crash really was the JIT probe executing code at startup. Confirmed on the same run: `cores: 8 of 8 declared`, `Apple A19 Pro GPU / Metal`, `library: 17 game(s) of 19 file(s)`, and every cover present |
| ☑ **1b. The JIT entitlement** | — | — | **ANSWERED, and the answer is no.** The app read its own signature: `get-task-allow is MISSING, so nothing can attach a debugger and no recompiler can run.` Expected for a distribution certificate, which cannot carry that entitlement. Also reported `MAP_JIT refused, plain executable mapping accepted`, which is a mapping result and not permission to execute. **Do not press the execute button** — without `get-task-allow` it can only close the app |
| ☐ **1c. Development signing, when you feel like it** | Enable Settings → Privacy & Security → **Developer Mode**, reboot, then re-sign with the Development cert | Installs, and the JIT line changes to "get-task-allow is present" | Development signing failed with an integrity/verification error, and the usual cause is Developer Mode being off: iOS has refused development-signed apps without it since iOS 16. StikDebug needs it enabled too. **Nothing you can play depends on this** — it only matters for N64, PSP, 3DS and Switch, none of which are in the app |
| ☐ **2. Save state slots** | Save twice in one game, then open that game's card and load the older one | Both saves listed newest first, and loading either works | Loading used to fail on some games and not others. That was my bug: I refused any state whose length was not exactly what the core reported at that instant, and cores are allowed to change that figure. Now only a state **shorter** than the core needs is refused |
| ☐ **3. The layout editor** | Settings → Move the on-screen controls. Try to drag a group. Read the **DRAGGING** line | The groups move, and DRAGGING counts up | You have reported this broken three times and I have stopped guessing. **The DRAGGING count is the whole point:** if it never moves, the touches are not reaching the pad. If it counts up while nothing moves, they arrive and the redraw is broken. Those need opposite fixes |
| ☐ **4. Cover from the game** | In the player, tap **⋯** → Use this frame as the cover | That game's cover becomes the frame you were on | Last time there was no button, because it was hidden behind a long press. A plain tap opens the menu now. Pausing first on a title screen is the best way to use it |
| ☐ **5. The selectors** | Look at Picture, Fast forward, Rewind in Settings | Rounded pill, like the system's own, not a square block | You told me twice. The first fix changed the material and left the corner radius, so the shape did not change. They are capsules now |
| ☐ **6. Auto-hide the pad** | Connect a controller, then Settings → Hide the on-screen pad | On-screen pad disappears, picture takes the space | Reported as not working, and nothing is wrong: it cannot act until a controller is attached, and it was tested without one. It now says **On, but waiting** in that state |
| ☐ **7. A DS game** | Import a `.nds` file and tap it | Boots straight into the game, with **both screens** visible, one above the other | Brand new: the sixth core. melonDS is software rendered on iOS, so the DS needed none of the graphics work the N64 needs. Its framebuffer is 256x384, which is both screens already stacked, so the existing compositor should draw them with no changes. **No BIOS files needed** — see the note below |
| ☐ **8. The DS touch screen** | In a DS game, tap and drag on the **lower** screen | The game responds where you touched, and dragging drags | The control the DS is defined by, and it is new in this build. Only the lower screen responds, which is the hardware: the top screen was never a digitiser. If touches land in the wrong place, **say whether they were offset by a little or landed on the wrong screen entirely** — those are different bugs. If nothing happens at all, say whether the buttons still work |
| ☐ **9. Two new systems, no new emulators** | Import a `.sg` (Sega SG-1000) file and tap it | Boots, on its own shelf called Sega SG-1000 | Reading the six cores' own lists of supported file types showed the app was refusing files it could already run. The SG-1000 needs nothing extra, so this is the one to try first |
| ☐ **10. A `.smd` Mega Drive file** | Import a `.smd` and tap it | Plays like any other Mega Drive game | Same cause. `.smd` is an older but very common Mega Drive format and the app was turning it away. Also newly accepted: `.swc` and `.fig` for SNES, `.unf`/`.unif` for NES, `.sgb`, and `.mdf`/`.toc` for PlayStation |
| ☐ **11. Famicom Disk System** | Import a `.fds` file and tap it | Either it plays, or it names `disksys.rom` | The Disk System cannot start without a startup file that is Nintendo's own code, so the app cannot include it. If you have not added that file, the message should tell you the exact filename to put in the Continuum folder. **Getting that message is a pass** — it means the check works |
| ☐ **12. TurboGrafx-16** | Import a `.pce` file and tap it | Plays, own shelf called TurboGrafx-16 | A seventh core, and it needs nothing from you. Only HuCard games: PC Engine CD needs a BIOS that cannot ship. If the two buttons feel swapped, tell me — I took I and II from the core's own list rather than from the names, and they are the opposite way round from how they read |
| ☐ **13. Atari 2600** | Import a `.a26` file and tap it | Plays, one FIRE button plus SELECT and RESET | An eighth core, also needing nothing. **It must be named `.a26`, not `.bin`** — rename it if yours is `.bin`, because `.bin` belongs to PlayStation discs here. This is also the first core that needed the game loaded into memory rather than opened from disk, so if it shows a black screen say so: that would be the new code path and not the emulator |
| ☐ **14. NINTENDO 64** | Import a `.n64`, `.z64` or `.v64` file and tap it | It boots and you can move | **Expect it to be SLOW.** This core renders in software and interprets every instruction, because that is the only way the N64 runs without the JIT permission we do not have. Slow is the expected result; a black screen or a crash is not. The D-pad surface drives the **analog Control Stick**, not just the D-pad, because almost no N64 game reads the D-pad — Mario would not move otherwise. Tell me roughly what frame rate the counter shows |

### Answered without a device: the DS needs no BIOS files

This was question 8 in the queue and it is now settled by reading the core's own source, so it
does not need a test of its own.

**No `bios7.bin`, `bios9.bin` or `firmware.bin` required.** This build of melonDS carries a
FreeBIOS and generates a default firmware when the real dumps are absent. Settings → BIOS still
lists those three names and reports which are present, and it is fine for it to say none.

Finding that out turned up two faults that would each have cost a wasted test, both the same
mistake in different clothes. This app refuses to answer a core's requests for its settings, on
the principle that a core's own defaults are better than values a frontend invents. But two of
melonDS's settings do not start at the default they advertise; they start at whatever their C
variable was initialised to, and the advertised default is only ever applied by a frontend that
answers. So:

- the **touch screen** advertises mouse control and starts at *disabled*, which meant the screen
  was switched off inside the core and no amount of correct data from the app could have reached
  it;
- **boot game directly** advertises enabled and starts at *off*, which would have sent the core to
  the DS firmware menu. A generated firmware has no menu that can launch a cartridge, so a game
  would have loaded and then sat there.

Both are now answered explicitly for this one core, and every other setting of every core is still
left alone.

### Fixed since the last build: the mgba flake

**Diagnosed, so this should stop happening.** One build produced an mgba core with no emulator API
in it at all, and the next build of the identical commit was fine. The cause was link-time
optimisation being allowed to delete the very functions the app looks for, because nothing in that
link mentioned them by name, and whether it deleted them depended on how the compiler happened to
split the work across cores. They are now named explicitly, so there is nothing left to chance.

Two things follow from it that are worth knowing:

- The build used to check that a core had **one** of the twenty functions the app needs. It now
  checks all twenty. A core that loses one of them fails the build instead of failing on your
  phone, which is what happened for months with save states.
- If a build ever does fail mentioning a core and a missing entry point, that is this check doing
  its job. Tell me which core and I will look at it; a retry is no longer the expected fix.

### Cannot be tested on purpose

**The save-state compatibility refusal.** It only fires when a state is loaded by a different core,
or by a different build of the same core, than the one that wrote it. You would have to engineer
that. It is what stops a state loading "successfully" into a game whose insides are then quietly
wrong, so it matters, but there is no reasonable way to ask you to trigger it.

### Never tried at all

`.sms` and `.gb` files. Every other extension has been imported and played. See
[Test 3](#test-3-the-other-cartridge-systems).

---

## What is already confirmed working

This build has been run on an iPhone 17 Pro Max and it plays games. Five cores each ran a real
game at 60 fps with 0 dropped frames. The sixth core, the DS, was added later and is still in the
queue above, so the `cores:` line now reads **6 of 6 declared** rather than the 5 of 5 that these
runs showed:

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

**Already passed**, when there were five cores. It reported 5 of 5 declared; with the DS added it
should now say 6 of 6.

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
- The `cores:` line should say **6 of 6 declared**.
- Below the text block there is a **Library** heading with an **Import Games** button.

**Tell me if it did not work**

- Whether it failed to install, or installed but would not open, or opened and closed again.
- **Which installer you used.** This matters more than it sounds like it should, because
  different installers keep or strip the app's special permissions, and that changes what is
  likely wrong.
- If it opened: the screenshot.
- If the `cores:` line says fewer than 6, send that whole line word for word. It names the
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
| `cores:` | How many of the six emulator cores are actually inside the app. Should be 6 of 6. |
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
