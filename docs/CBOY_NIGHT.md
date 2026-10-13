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

## For whoever is editing the notes

STATUS.md and HANDOFF.md still say mosaic is missing. Three places to fix:

- STATUS.md line 76: the Advance feature list ends "mosaic not done" — mosaic is done now.
- STATUS.md line 79: "Not done yet" lists mosaic — drop it from that list.
- STATUS.md "Next up" item 6 and HANDOFF.md's checklist both list "Advance mosaic" as a job —
  tick it off.

## Phone checks for the owner

1. Open any Advance game on cBoy (pick cBoy, not mGBA, in the game's settings) and start a battle
   or walk through a door. Where the screen changes it should break into big squares for a moment,
   not just fade or cut.
2. In a Pokémon game on cBoy, surf out onto the water and watch the edges of the land. The
   underwater wobble should look chunky, not smooth.
3. Play anything on cBoy for a few minutes and check nothing else looks blocky when it should not.
   If a picture that used to be sharp is now chunky all the time, that is this change gone wrong —
   say so and it gets pulled straight out.
