# cBoy, the night of 12-13 October 2026

What one agent did to cBoy (Continuum's own Game Boy / Color / Advance emulator) overnight, in
plain words. Everything here is **built and tested in the test suite, but no phone has seen it
yet** — the build of the night comes at the end.

Three agents worked at the same time, so this file carries anything the notes (STATUS.md,
TESTING.md, HANDOFF.md) need, instead of editing them.

## What landed

### Game Boy Advance mosaic — built, untested on a phone

Mosaic is the chunky-pixel effect: the picture is split into blocks and every pixel in a block
takes the colour of the block's top-left pixel. Advance games use it constantly — screens
dissolving into big squares when you enter a battle, the wobble when you go underwater, a
character pixelating away. cBoy ignored the setting completely until now, so those moments just
looked flat and wrong.

It now works on every kind of layer:

- Plain scrolling backgrounds. The blocks sit still on the screen while the background slides
  through them, the way the hardware does it.
- Rotated and scaled backgrounds. The sideways blocking happens before the rotation maths, and
  the up-and-down blocking holds the layer's position for the whole block of lines, so a spinning
  or zooming layer goes chunky properly instead of staying smooth at its edges (a thing mGBA
  itself still gets wrong, their issue 1691).
- The three photo-style (bitmap) picture modes, which run through the same path.
- Moving objects (sprites), each with its own mosaic switch. Up and down, an object is drawn as it
  was on the first line of its block. Sideways, the blocking is done on the finished line of
  objects, which is where the real hardware does it: one pixel is held and repeated, and it is
  dropped the moment a plain object or empty screen turns up next to a chunky one. So a mosaic
  character no longer smears across the plain ones beside it.

### What the tests prove

Four new tests in the core (82 tests pass in total, up from 78):

1. With the setting at zero, or with a layer's own mosaic switch off, the picture comes out
   **exactly** as it did before — pixel for pixel, on all four kinds of layer. No game that never
   touches mosaic can be changed by this.
2. Backgrounds (plain, rotated, bitmap): a known pattern is drawn twice, once plainly and once
   with mosaic, and the chunky picture must match the plain one blocked up by hand. The sizes used
   across and down are different (4 by 2 for backgrounds, 8 by 4 for objects), so the test cannot
   pass if the two directions, or backgrounds and objects, were mixed up.
3. Objects: the same check, plus one for the edges — a plain object sitting right next to a chunky
   one has to come out untouched.

### What was deliberately left

One hardware detail is not copied: on a real Advance, the see-through pixels inside a mosaic
object still count as "this is a mosaic object" while the line is being blocked up, so a hole in a
chunky sprite gets filled in by the held pixel rather than showing what is behind it. Copying that
needs the object layer to remember see-through pixels as well, which is tangled up with a separate
old hardware quirk (the "sprite priority bug") that cBoy does not copy either. It only shows up on
a sprite with holes in it while it is mosaicked; nothing noticeable in a real game. Written down
here so it is not lost.

### Four new settings for cBoy — built, untested on a phone

cBoy keeps a list of every setting any well-known Game Boy or Advance emulator offers, and four
of them were still missing. All four are in now, in cBoy's own Core settings, under System.

**Console for games that run on both.** A lot of cartridges work on a plain Game Boy *and* on a
Game Boy Color — Pokemon Yellow, Link's Awakening DX, loads of them. cBoy always put those on a
Color. Now you choose: Color (still the default, so nothing changes unless you want it to), plain
Game Boy if you want the four-shade look and your own palette, Super Game Boy if you want its
frame round the picture, or Advance. The console you pick also changes how the game starts up
underneath, the way the real machines do, so a game that checks which console it is in gets the
right answer.

**Advance console.** Game Boy Advance, Advance SP, or Game Boy Player (the one that plugs into a
GameCube). Picking the Player makes Advance games that rumble through it rumble the phone. The
Advance and the SP are the same machine as far as a game can tell — only their screens looked
different, and that is already a Picture setting — so the setting says so in plain words rather
than pretending otherwise.

**Advance cartridge clock.** Some Advance cartridges have a real clock in them (Pokemon Ruby and
Sapphire, Boktai). Until now cBoy always gave them the phone's time. Now there is a second
choice, "Game time only": the clock starts at the phone's time and then only moves while you are
actually playing. Handy if you do not want a game's day and night to run on while the app is
closed. The phone's clock stays the default.

**Old fan-made game fixes.** A fair number of fan-made Advance games were built and tested on an
old emulator, VisualBoy Advance, which handled one thing differently from a real console: when a
game asks it to unpack more graphics than it left room for, VisualBoy stopped at the room asked
for, while a real Advance writes the extra bytes anyway and scribbles over whatever was next to
it. Those games can look broken on an accurate emulator. The setting is on by default (which is
what cBoy has always done, so nothing changes), and turning it off gets you the real console's
behaviour. A properly made game plays exactly the same either way.

### What the tests prove

Five new tests in the core (87 pass in total, up from 82):

1. Every value of all four settings is read correctly, and the defaults still come out exactly as
   they did before — so nobody's existing games can change by surprise.
2. "Runs on both" moves only cartridges that really do run on both: a Color-only game still gets
   a Color and a Game Boy-only game still gets a Game Boy, whichever way the setting is turned.
   It is also driven through the real emulator plumbing on a made-up both-consoles cartridge: on
   Game Boy every pixel comes from the four shades you picked, on Super Game Boy the picture comes
   out 256 by 224 with its frame, and on Color and Advance it stays in colour.
3. The Game Boy Player handshake is answered for the Player and for nobody else.
4. The game-time clock starts at the phone's time and then moves exactly as far as the game has
   run (an hour, a minute and a second of play moves it by an hour, a minute and a second), and
   the existing clock-offset setting still shifts it.
5. The unpacking test feeds the console deliberately bad data: with the fix on it stops dead at
   the size the data declared, with it off the extra bytes land in memory, and nothing else moves
   either way.

### What was deliberately left

- The game-time clock is remembered inside a save state, but not across closing the app: open the
  game again tomorrow and the clock is set to the phone's time again before it starts counting
  play time. Keeping it across sessions means a new little file next to the save, and changing
  what cBoy stores was not worth the risk tonight.
- "Advance console" does not make the SP behave differently from the plain Advance, because the
  real machines do not either (bar the screen). If a reason ever turns up, the setting is already
  there to hang it on.

### Six small things from the clean-up, fixed — built, untested on a phone

The October clean-up left a list of small cBoy problems. These six are done.

**A Game Boy game could be started as an Advance game.** cBoy decided a file was an Advance game
from a single byte, and on a Game Boy cartridge that byte is ordinary code that can hold the same
value by chance. When it did, the game came up as a 240 by 160 Advance and could not be played at
all, with no setting to force it back. Now a file that carries a Game Boy cartridge header (the
Nintendo logo the console itself checks, or a header sum that adds up) has to pass the Advance
header's own check as well before it is treated as an Advance game. Real Advance games, including
home-made ones that were never finished off properly, are unaffected.

**Settings that said "next start" were changing things straight away.** The sound rate and the
border were applied to the running game, but the app sets the sound up and sizes the picture once,
when the game opens, and keeps what it was told. So picking the lowest sound rate mid-game played
it back too fast and crackly, and switching the border on squashed the picture by about 3%. Both
now hold what the game started with, and their lines say "Takes effect when the game restarts",
which is also what makes the app offer you its "Restart the game now" button for them.

**A cartridge cBoy can't play now says so in words.** Before, it said nothing at all and you got
"the game file was refused: retro_load_game rejected the content". Now it says, for example, "This
cartridge has a chip cBoy cannot play yet (MBC6). Try this game on mGBA instead: open its settings
and pick mGBA as the emulator." Same for a file that is too small to be a game. The handful of
cartridges this covers are MMM01, MBC6, MBC4 and the Tamagotchi one.

**A game left talking to a cable with nothing on the end filled the phone's memory.** Every byte a
game sent was kept for ever, and a game that spins waiting for a friend sent about 470 MB an hour
— enough for iOS to shut the app down. The log now stops at 8 KB, keeping the first bytes, which
is all the test ROMs ever read.

**Boktai is playable again.** The "Sunlight" setting's "Light sensor" choice asks the phone how
bright it is, the app turns that request down (no iPhone offers a light reading), and the answer
was always nothing — so the sun sensor in Boktai's cartridge sat in permanent darkness and the game
could not be played. When there is no reading to be had it now sits at half sun, and the setting
says so. Picking a fixed level yourself still works exactly as before.

**cBoy's settings page is in plain words, and the layer switches are out of it.** The page used to
show a "Debug" group of thirteen switches, one per part of the picture: turning "Sprites" off makes
a game look broken, and no player has a reason to touch them. The ten layer switches are now hidden
from the page (the app's own "Show settings the core hid for now" switch still brings them back for
anyone hunting a drawing problem), and the two that are about how the real console looked —
"Sprite flicker" and "Advance sprite flicker" — moved into Picture, where they belong, with plain
descriptions. The jargon went too: "DirectSound", "output capacitor", "aliasing", "256x224",
"handshake" and a colour written as #E0F8D0 are all gone, and "Game Boy Color revision", which
offered five choices that behaved identically, is now "Which Game Boy Color" with the only two that
differ.

### What the tests prove

Ten new tests in the core (95 pass in total, up from 85):

1. A Game Boy cartridge with that Advance marker byte in its code stays a Game Boy game, with its
   logo and without it; an Advance cartridge is still an Advance game, including one whose own
   check byte was never put right; and the real Advance test ROM still comes up as Advance.
2. The sound rate and the border, changed while the game runs, change nothing: the same number of
   sound samples comes out and the picture keeps its size. Chosen before the game starts, both
   still do exactly what they say. Everything else (the palette) still changes on the spot.
3. Every unsupported cartridge type is refused with a sentence that names the chip in human terms,
   points at mGBA, and contains no codes; every type cBoy does play is still accepted.
4. A game sending down the cable for ever fills the log to its limit and then stops, keeping the
   first bytes sent.
5. The sunlight level: a level you picked is used as it stands; a real reading runs from dark to
   full sun; with no reading to be had it is half.
6. Every one of the ten layer switches is hidden from the settings page, nothing else is, the
   frontend is told it can ask again, and the settings a player has a use for stay visible.
7. Nothing a player reads in cBoy's settings contains any of the jargon words the owner picked out
   — so it cannot creep back in.

### What was deliberately left

- Eleven other cBoy settings still end with "Next start." rather than the word "restart". They do
  wait, and always have (they only matter while the machine is being built), but because they do
  not use the word, the app does not offer its "Restart the game now" button for them, and you have
  to close and reopen the game yourself. Changing all eleven also changes how the app holds them
  mid-session, which is a bigger decision than this clean-up item: worth doing, but as its own
  piece of work with the owner's say-so.
- The Debug group is hidden while a cBoy game is running, which is where the settings page is
  normally opened from. Opened from the emulator list with no game running, the app lists cBoy's
  settings from what it remembers, and the layer switches show up again. Closing that needs a change
  in the engine's own options code (`crates/emulator-bridge/src/cores/options.rs`), which belongs
  to another agent tonight: either remember what a core hid alongside the table it caches, or add
  cBoy's ten `cboy_layer_*` keys to that file's own hidden list.

### The phone's camera into the Game Boy Camera — built, untested on a phone

The Game Boy Camera was a cartridge with a little camera on a swivel. cBoy already pretended to
have the cartridge, but nothing was ever plugged into it: the code had a slot for "the last
picture from the phone" that nothing on earth filled, so every photo you took came out as four
fixed stripes. The phone's camera now reaches it, end to end.

What was already there and what was missing: the app's side was finished and working (it is what
feeds the 3DS camera — the capture session in Swift, and the engine's own camera code in
`crates/emulator-bridge/src/peripherals/camera.rs`). The missing half was cBoy itself, which
never asked for a camera. It does now:

- As a Game Boy Camera game loads, and **only** then, cBoy asks the app for the camera, at 128 by
  120 — the size the real camera chip worked in. No other game asks, so opening Tetris will never
  make the phone ask for camera permission.
- On the first frame it switches the camera on. Each frame after that, the newest picture is
  handed over, shrunk to the chip's size and turned to shades of grey (the real chip only ever saw
  brightness, never colour).
- Taking a photo in the game now runs the picture through what the cartridge actually did, in the
  hardware's own order: the exposure time the game chose, its narrow brightness range, the invert
  switch, the edge-sharpening the game asked for, and then the contrast-and-dithering grid the
  game programs — 16 little cells of three thresholds each, which is what turns shades of grey
  into the Game Boy's four colours and gives Camera photos their speckled look.
- The camera goes off when the game closes, rather than when the app next notices.
- With no camera — the app has none, or "Allow camera in games" is off — the game sees an even
  grey and carries on. No made-up picture, ever.

Also fixed while in there (this closes finding CBOY-11, both halves):

- **The photo was landing 256 bytes too early.** Pan Docs is clear that the cartridge writes the
  picture into save RAM 256 bytes in, and cBoy was writing it at the very start, on top of the
  game's own first 256 bytes. Anyone who had used the camera had those bytes quietly scribbled on.
- The comments that claimed a phone picture it never got are gone, because it gets one now.
- Smaller things the real cartridge does that cBoy did not: only the trigger register reads back
  (and only its bottom three bits), the registers repeat every 128 bytes through the window, any
  bank number with bit 4 set reaches the registers rather than only bank 16, and while a photo is
  being taken the cartridge's RAM goes unreadable and ignores writes, which is how the game knows
  it is still working.

### What the tests prove

Six new tests (100 tests pass in total, up from 95). Two of them drive the whole thing through the
real libretro entry points, with a made-up camera frame and a Game Boy Camera cartridge built
inside the test, so nothing needs downloading:

1. End to end: the core asks for a camera once, at 128 by 120, before the game runs; starts it on
   the first frame; a frame handed over turns into the right four shades in the cartridge's RAM at
   the right address; a second, different frame replaces it; and the camera is stopped when the
   game closes.
2. The game's own first 256 bytes are still intact after a photo — the CBOY-11 bug, pinned so it
   cannot come back.
3. No camera, no harm: an app with no camera to give, and an app whose camera switch is off, both
   leave the game running and the picture an even grey. An ordinary cartridge (MBC5) never asks
   for a camera at all.
4. The four rows the cartridge throws away really are thrown away: the test frame has its top and
   bottom filled with the opposite shade, and none of it reaches the picture.
5. The dithering grid genuinely dithers: one flat grey, with every other cell of the grid given a
   different threshold, has to come out striped.
6. The invert switch turns the darkest band into the lightest and the lightest into the darkest.

### What was deliberately left

- **The photo is taken instantly.** On the real cartridge it takes a measurable time, and Pan Docs
  gives the sum (`4 * (32446 + (N ? 0 : 512) + 16 * exposure)` T-cycles). cBoy finishes it in a
  couple of ticks instead. Games only poll a bit of the trigger register until it clears, so they
  work either way, and nothing visible is wrong. It is left alone because the countdown needs a
  wider number than the one-byte one in the save state, and widening it changes the save-state
  layout — which means the tag has to be bumped and **every save state anyone already has stops
  loading**. That is the owner's call, not a thing to slip into a clean-up: worth doing, but as
  its own piece of work, together with any other state change.
- No new setting was added for the camera. The app's existing "Allow camera in games" switch and
  iOS's own permission already cover it, and the camera chip's own knobs are the game's to set.

### The Game Boy Color test ROMs — 4 of the 5 now pass, and they are a real gate

Mooneye (the strictest public Game Boy test suite) has five tests written for Color hardware.
cBoy passed none of them, and nothing stopped that getting worse: they were only printed as a
count, not checked. Four pass now and the test fails the build if any of them ever stops passing.
The fifth is still only printed, and why is below.

Two of the five were never really broken: the harness was starting them wrong. All five are Game
Boy cartridges, on purpose — what they measure is **a Color console playing a Game Boy game**,
which is a thing of its own, not the same as a Color playing a colour game. The harness was
starting them as colour games, so they were handed the wrong start-up registers and failed on the
first line. Started the way cBoy's own "Console for games that run on both" setting starts them,
they pass as they are.

The other two were genuinely wrong, and this is the real fix. A Color playing a Game Boy game is
still Color hardware, and a handful of its registers answer even though the game is in Game Boy
mode. cBoy treated that machine as a plain Game Boy, so those registers were missing:

- the video bank register answers, stuck on the first bank (and the game cannot escape to the
  second one by writing it);
- the two colour-palette pointers answer, holding what the console's start-up code left in them
  part-way through picking a palette for the game, while the palettes behind them stay shut;
- the Color's three scratch bytes ($FF72, $FF73 and the three middle bits of $FF75) answer;
- the two sound read-back bytes answer with what the four sound channels are actually putting
  out, two channels to a byte — cBoy now works that out from the real channel levels rather than
  reporting a fixed number;
- the pad register starts with neither half of the pad selected, which is what the Color's
  start-up code leaves behind and the opposite of a Game Boy's;
- and the registers that belong to colour mode — double speed, the block copier, the palettes
  themselves, sprite order, the work-RAM bank — stay shut away, which is what the hardware does.

None of this changes anything for a colour game or for a Game Boy game on a Game Boy: the new
behaviour only appears when the console is set to Game Boy Color or Advance **and** the cartridge
is a Game Boy one. The 86 Game Boy tests still pass on both picture drawers.

### What the tests prove

1. Four Mooneye Color tests pass and are gated, six runs in all: the two register ones are run as
   a Color and as an Advance, because the hardware answers the same on both.
2. A Game Boy game on a Color sees every register above with the right value, and the colour-mode
   ones still read as absent.
3. The same game on a Game Boy sees none of them.
4. Writing the video bank register in Game Boy mode cannot reach the second bank.
5. The sound read-back really follows the sound chip: start the wave channel on a known sample and
   the right four bits change, the other channel's stay silent, and turning the sound off zeroes
   both.
6. Which console it is survives a loaded save state (it comes from the player's setting, not from
   the save).
7. **No save state anyone already has is affected.** A state is still byte-for-byte the same size,
   because the two new pieces of machine state are deliberately kept out of it.

### What was deliberately left

- **The fifth test (`vblank_stat_intr-C`) still fails, and it can only be fixed in `ppu.rs`,
  which another agent is rewriting tonight.** What it needs, for whoever has that file: on a
  Color, the mode 2 (sprite-scan) interrupt at the start of line 144 arrives **one M-cycle before**
  the VBlank interrupt; on a Game Boy the two arrive together. cBoy has the Game Boy behaviour for
  both, in the STAT interrupt condition in `ppu.rs` (the `self.ly == 144 && self.dot < 4` term):
  the window needs to sit one M-cycle earlier when the console is a Color, which is now readable
  as `bus.cgb_hardware`. The Game Boy version of the very same test
  (`acceptance/ppu/vblank_stat_intr-GS`) is in the gated set and passes, so the Game Boy side must
  not move. Once it is right, move the ROM from the reported line at the bottom of `mooneye_cgb`
  in `tests/roms.rs` into the gated list above it.
- The three scratch bytes are not written into save states. Putting them in would change the
  save-state layout and **stop every save state anyone already has from loading** (bincode cannot
  skip a field it has never seen — that was checked, not assumed). Nothing reads them back across
  a save, so they start again from zero after a state is loaded. Same reasoning as the camera's
  capture countdown above.
- $FF74, the Color's fourth scratch byte, is left absent. No test measures it and no game is known
  to use it, so nothing was invented for it.

### Real Advance games, not just test ROMs — and the frozen-screen bug they found

Until tonight cBoy's Advance side had only ever been given test ROMs: little programs that check
one thing and print a pass or a fail. Six **real homebrew games** now run in the test suite, all
the way through the same plumbing the app uses, with the buttons being pressed on a timetable. They
are all published free by their authors under open licences; none is a commercial cartridge, and
none of them is kept in the repository — they are downloaded when the tests are run, into a folder
git ignores.

The games, and what happened:

| Game | What it is | What happened |
| --- | --- | --- |
| **Pong** (ZeroDayArcade) | a tiny 9 KB Pong | Was a blank screen. Now plays: both bats, the ball, the scores and the "P1 / CPU" labels. Silent game, no sound to lose. |
| **Wigly Hunt** (rawbits2010) | a Duck Hunt-style jam game | Was a blank screen with the music playing behind it. Now draws the sky and the duck, with its music. |
| **Everything Counts** (Godless-1) | a story game | Worked already, and still does: its night-time city scene, speech box and music all came out right. |
| **Attack on Voxelburg** (Mutant Stargoat) | a 3D voxel shooter | Worked already: its title screen draws in 264 colours and its menu answers the buttons. |
| **Symbol Merged** (copyrat90) | a puzzle-platformer built with butano | Was frozen and silent. Now plays its level, with its music, and fades between screens. |
| **Skyland** (Evan Bowman) | a big strategy game, about 3 MB | Was frozen and silent. Now gets through its own Game Boy Player splash screen, into its menu and on to its island-and-lighthouse scene, with sound. |

**The bug: every game that waits for the screen the usual way sat frozen for ever.** Four of the
six never got past their first frame, and this is why. A Game Boy Advance game normally does its
work and then asks the console's built-in software to "wake me when the screen has finished
drawing" — the call every ready-made toolkit uses (libgba, butano, tonc), so most Advance games
alive use it. The call means two things: *throw away any screen-finish that already happened*, then
*wait for a new one*. cBoy's copy of that call threw the old one away **every time it looked**,
including the look that came right after the interrupt it had been waiting for — so it threw away
the very thing it wanted and went back to sleep, for ever. The game's sound and interrupts carried
on (which is why two of them played music to a blank screen), but no game logic ever ran again.
It now throws away only on the call itself, and the waits end on the screen-finish they were
waiting for.

This never showed up before because test ROMs do not wait like that: they poll the line counter
or just run flat out. It is the single biggest Advance fix of the night: without it, a large share
of real Advance games could never have worked in cBoy at all.

### What the tests prove

Two new tests (106 pass in total, up from 104):

1. The wait itself, checked step by step with no game needed, so it is gated even on a machine
   with no network: a screen-finish that happened before the call is thrown away; looking again
   with nothing new keeps waiting; the moment the interrupt's own handler notes a new one, the
   wait ends and takes it; and the *next* call is a new call again and throws an older one away.
   Waiting for any other interrupt is checked too, both ways round ("throw the old one away" and
   "do not"). Put the old code back and this test fails — that was checked, not assumed.
2. The six games, run through the real libretro entry points for 300 frames each (900 for
   Skyland, which spends its first 500 on its own splash screen), with A, Start and the pad
   pressed on a fixed timetable. Each one has to:
   - never panic, and never refuse to load;
   - send a 240x160 picture, with the right row length, on every single frame, in the colour
     format the app was promised — one picture per frame, none dropped;
   - keep drawing **new** pictures (a frozen game draws one picture for ever, which is exactly
     what three of these did before the fix);
   - draw a rich enough picture at some point in the run (the fewest colours allowed is about
     half what each game really draws, so a game going blank fails but a few pixels moving does
     not);
   - hand over a full frame's worth of sound every frame, and the four games that have music must
     really make some (so a broken sound engine cannot pass quietly);
   - and **survive a save state taken half way through**: the state is loaded back, the second
     half is replayed with the same buttons, and the game has to come out on exactly the same
     picture, pixel for pixel.

Every picture above was looked at by eye as well, not just counted.

### What was deliberately left

- **Skyland's splash screen takes about three times as many frames as it should.** Its own code
  waits for the screen 50 times there; cBoy takes about 190 frames to get through it, so cBoy is
  giving the game roughly a third of the work per frame that a real Advance would. That points at
  the cartridge read timing — the console has a small read-ahead buffer (the "prefetch"), and
  cBoy only approximates it (it hides the waiting for runs of reads when the game asks for it,
  rather than modelling the buffer). Nothing looks or sounds wrong because of it, and all six
  games play, but a game that leans on the console's exact speed could run slow. Measuring it
  properly needs a timing test ROM and is its own piece of work, so it is written down rather
  than guessed at.
- No Advance game is added to the committed test ROMs. The rule stands: fetch, never commit.
- The two games that are silent (Pong and Voxelburg, in the parts the test reaches) are marked as
  silent in the test, rather than pretending they prove anything about sound.

## For whoever is editing the notes

STATUS.md and HANDOFF.md still say mosaic is missing. Three places to fix:

- STATUS.md line 76: the Advance feature list ends "mosaic not done" — mosaic is done now.
- STATUS.md line 79: "Not done yet" lists mosaic — drop it from that list.
- STATUS.md "Next up" item 6 and HANDOFF.md's checklist both list "Advance mosaic" as a job —
  tick it off.

The same two lists have "the four missing Advance and model settings" as a job — that is done
too. cBoy now has 66 settings of its own, and the list of settings other emulators have that it
does not is down from 23 to 22 on the libretro side and from 14 to 9 on the standalone side
(docs/CBOY_OPTIONS_INVENTORY.md is regenerated and says so).

Seven of the clean-up findings are fixed and can be closed in
docs/reports/cleanup-2026-10-12/: CBOY-05 (a Game Boy ROM run as an Advance game), CBOY-06
("next start" options applying live), CBOY-07 (a code-like refusal), CBOY-08 (the serial log
growing for ever), CBOY-10 (the sunlight choice always dark), CBOY-11 (the Game Boy Camera
claiming a phone picture it never got, and the 256-byte offset — both halves confirmed against
Pan Docs and fixed) and CBOY-12 (jargon and debug switches in the settings page). CBOY-01 to
CBOY-04 and CBOY-09 are untouched so far.

The same two lists have "the phone's camera into the Game Boy Camera" as a job under item 6 —
that is done too, and the Game Boy Camera can now go in a tester-facing list of what cBoy plays
(CBOY-11 said to keep it out until it really worked; it does).

Nothing is needed from the Swift side for the camera: the capture session in
`native/ios/Peripherals.swift` already drives any core that asks, and cBoy now asks. No change is
needed in `fifo.rs` or `ppu.rs` either.

The same two lists have "the Mooneye Game Boy Color tests (0 of 5)" as a job under item 6. It now
reads 4 of 5, gated like the Game Boy ones, with the fifth waiting on `ppu.rs` (see above).

The same two lists have "real Advance games tried, not only test ROMs" as a job under item 6 —
that is done, and it found the big one: most real Advance games could not run at all, because the
"wake me when the screen is drawn" call never finished (see above). **STATUS.md's line that the
Advance side has only ever run test ROMs is now out of date**: six real homebrew games are in the
suite and gated. If the Advance side is described anywhere as unproven on real games, it can say
instead that six open-licence homebrew games run headless in the tests, pictures, sound and save
states checked, with phone testing still to come. Where TESTING.md or STATUS.md quotes a cBoy
test count, the suite is 106 tests, up from 100 at the start of the night.

## Phone checks for the owner

1. Open any Advance game on cBoy (pick cBoy, not mGBA, in the game's settings) and start a battle
   or walk through a door. Where the screen changes it should break into big squares for a moment,
   not just fade or cut.
2. In a Pokémon game on cBoy, surf out onto the water and watch the edges of the land. The
   underwater wobble should look chunky, not smooth.
3. Play anything on cBoy for a few minutes and check nothing else looks blocky when it should not.
   If a picture that used to be sharp is now chunky all the time, that is this change gone wrong —
   say so and it gets pulled straight out.
4. Open Pokemon Yellow (or any Game Boy game with a colour-screen version) on cBoy. It should look
   the same as it always has. Then in cBoy's settings set "Console for games that run on both" to
   Game Boy and restart the game: it should go to four shades, and your Game Boy palette should
   apply. Set it to Super Game Boy and restart: a frame should appear round the picture. Put it
   back to Game Boy Color when you are done.
5. Open an Advance game on cBoy and set "Advance console" to Game Boy Player. The game should play
   normally. (If you have a game that rumbles through the Player, the phone should buzz.)
6. If you have Pokemon Ruby or Sapphire on cBoy, set "Advance cartridge clock" to Game time only,
   play for a few minutes, close the app for an hour, and come back: in-game time should have moved
   by the few minutes you played, not by the hour you were away.
7. Have a look down cBoy's System settings and tell me if any of the four new lines read oddly or
   sound like a computer wrote them. They are: "Console for games that run on both", "Advance
   console", "Advance cartridge clock" and "Old fan-made game fixes".
8. Open cBoy's settings with a cBoy game running and look at the whole page. There should be no
   "Debug" group of layer switches any more. Picture should now end with "Sprite flicker" and
   "Advance sprite flicker". Tell me if any line still reads like documentation.
9. In cBoy's Sound settings, change "Output sample rate" to 32768 Hz while a game is playing. The
   sound should carry on exactly as it was, and the app should offer you "Restart the game now" —
   tap it, and the game should come back sounding normal, not fast or crackly.
10. Play a Game Boy game on cBoy and check it still looks and sounds right — the Picture group
    gained two lines but nothing's default changed.
11. If you have an oddball cartridge cBoy can't play (a Tamagotchi game, Net de Get, a Momotaro
    collection), open it on cBoy. Instead of the old "retro_load_game rejected the content" you
    should get a sentence naming the chip and telling you to use mGBA.
12. If you have Boktai, open it on cBoy with Sunlight on "Light sensor". The game should play in
    half sunlight instead of pitch dark (before, it was unplayable).
13. Leave a cBoy game that uses the link cable (Pokemon, Tetris) sitting at its trade screen for a
    few minutes with nothing to link to. The app should stay put and not get killed for using too
    much memory.
14. If you have the Game Boy Camera, open it on cBoy and make sure "Allow camera in games" is on
    in the app's settings (iOS will ask for camera permission the first time — say yes). Go to
    Shoot in the game's menu. You should see what the phone's camera is pointing at, in four
    shades of grey and speckly, moving as you move the phone. Press the button to take a photo and
    it should keep the picture you were looking at.
15. Open any other cBoy game — Tetris, Pokemon, anything. The phone must **not** ask for camera
    permission. If it ever does for a game without a camera, that is wrong, tell me.
16. If you have the Game Boy Camera and turn "Allow camera in games" off, open it again: Shoot
    should show an even grey rather than a picture, and the game should still work normally (you
    can still move around its menus). Turn the switch back on afterwards.
17. Only if you had already saved photos in the Game Boy Camera before this build: open it and
    check your old album still opens and your old photos are still there. The place a photo gets
    written moved 256 bytes, which was a bug fix, but it is worth a look.
18. Open a plain Game Boy game (Tetris, Link's Awakening, a Pokemon) on cBoy and play a few
    minutes. It should look, sound and play exactly as it did before — this change was accuracy
    work deep inside, and if you can see or hear any difference at all, something is wrong and I
    want to know. Then load one of your older save states for the same game: it must still load.
19. **The big one for Advance games.** Open any Advance game on cBoy (pick cBoy, not mGBA, in the
    game's settings) and just play it for a couple of minutes. Before tonight most Advance games
    would have come up as a black or frozen screen on cBoy, sometimes with the music playing
    behind it; that is fixed, and this is the check that matters most. If any Advance game still
    sits on a blank or frozen screen, tell me which one.
20. With that Advance game running, make a save state, carry on playing for a minute, then load
    the state back. It should drop you exactly where you were and keep playing normally.
21. If an Advance game feels slower than it does on mGBA — slow motion, choppy where mGBA is
    smooth — tell me which game. Something known is a bit off in how fast cBoy lets Advance games
    read their cartridge, and knowing which games show it tells me whether it is worth chasing.
