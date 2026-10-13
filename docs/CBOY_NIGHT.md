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

Six of the clean-up findings are fixed and can be closed in
docs/reports/cleanup-2026-10-12/: CBOY-05 (a Game Boy ROM run as an Advance game), CBOY-06
("next start" options applying live), CBOY-07 (a code-like refusal), CBOY-08 (the serial log
growing for ever), CBOY-10 (the sunlight choice always dark) and CBOY-12 (jargon and debug
switches in the settings page). CBOY-01 to CBOY-04, CBOY-09 and CBOY-11 are untouched so far.

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
