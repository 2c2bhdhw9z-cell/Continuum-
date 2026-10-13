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
