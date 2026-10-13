# cBoy's exact pixel-by-pixel drawer

The core option "Picture drawing" offers two drawers. **Standard** (`render_line` in
`cores/continuum-gb/src/ppu.rs`) draws a whole line the moment mode 3 begins; it is the default
and the one every game here has been played on. **Exact (experimental)** (`fifo.rs`) walks mode 3
one dot at a time so a register written in the middle of a line lands on the right pixel.

This file is the working map for finishing Exact. It records what the hardware does, where cBoy's
model differs, and what each remaining test picture needs. It is not a user document; the part the
owner should read is the last section.

## Score

| Suite | Standard | Exact |
|---|---|---|
| dmg-acid2 | pass | pass |
| cgb-acid2 | pass | pass |
| Mooneye DMG acceptance | 86 / 86 | 86 / 86 |
| Mealybug Tearoom | not applicable | **23 / 55** |

All 23 are DMG pictures, which is **23 of the 24 DMG pictures**. The one that still fails is
`m3_lcdc_bg_en_change`, and it is **four pixels** away: one pixel on each of four lines, all of
them x=0, all of them on the eight lines where that test places a sprite at X=2. The 31 CGB
pictures are blocked outside this drawer, as Group 1 below explains, and 4 of those can never
reach zero at all.

`tests/roms.rs::MEALYBUG_PASSING` pins all 23 by name: a change that drops one of them is wrong,
whatever else it fixes.

### The two tools that make a picture into evidence

Never compare counts. Both of these are in `tests/roms.rs` and both were needed to place the
register-write dots below.

- `cargo test --release --test roms mealybug_report -- --nocapture` prints, for every failing
  picture, how many pixels differ, how many lines they fall on, the first differing line, the
  first differing pixel on it, and the colour drawn against the colour expected.
- `MEALY_DETAIL=<name fragment>` adds, for the pictures whose name contains that fragment, every
  one of the 144 lines as three rows of shade digits: drawn, expected, and a marker row.
- `MEALY_BASE=<name> MEALY_FIND=<bytes> MEALY_WITH=<bytes>` (the `mealy_baseline` test) runs one
  ROM with its mid-line write **neutralised by a patch of the same length and cycle count**, and
  prints the same digit rows. That is the picture the line would have drawn with its timing
  untouched, and it is what makes a reference picture readable: where the reference has colour 0
  and this picture has ink, the hardware suppressed that pixel; where the two agree, it did not.
  `MEALY_BASE=m3_lcdc_bg_en_change MEALY_FIND=cb,81 MEALY_WITH=cb,41` rewrites that test's
  `res 0,c` as `bit 0,c` - two bytes, eight cycles, no register touched - so all four of its LCDC
  writes write the value already in the register.

With the three together you get, for each of the eighteen sprite positions a test sweeps, the
exact first and last pixel of every window of suppressed pixels, in both pictures. A window that
is one pixel wide at the wrong place is then a fact, not a guess.

## Where the specification comes from

- **Mealybug Tearoom Tests** (MIT, `mattcurrie/mealybug-tearoom-tests`, pinned at
  `70e88fb90b59d19dfbb9c3ac36c64105202bb1f4` by `scripts/fetch-test-roms.sh`). Each test's `.asm`
  source opens with a comment saying which hardware behaviour it probes: that comment is the
  specification. The repository also ships
  [the comprehensive Game Boy PPU documentation](https://github.com/mattcurrie/mealybug-tearoom-tests/blob/master/the-comprehensive-game-boy-ppu-documentation.md),
  which states outright which fetch stage samples which register.
- **SameBoy** (MIT, `LIJI32/SameBoy`), `Core/display.c`. The emulator these pictures were written
  against. Read, never copied: the model below is described in hardware terms and implemented in
  cBoy's own Rust. Both are cloned into `/projects/sandbox/.scratch`, outside this repository, and
  are never committed.

Content from those sources is paraphrased here rather than quoted.

## The hardware model

### The background fetcher is six dots of work plus a push

Each of the fetcher's three VRAM accesses takes two dots. The **first** dot computes the address
from the registers; the **second** performs the read. Written as the Mealybug documentation names
them, one tile is `B`, `0`, `1` — tile number, bitplane 0, bitplane 1 — and then a push:

| Dot | Stage | What happens |
|---|---|---|
| 1 | `B` address | map base from LCDC bit 3 (background) or bit 6 (window); X from SCX bits 3-7 or the window tile counter; Y from SCY + LY or the window line |
| 2 | `B` read | tile number read; on the CGB the attribute byte is read in the same dot |
| 3 | `0` address | LCDC bit 4 (tile data select) sampled; Y re-read |
| 4 | `0` read | bitplane 0 read |
| 5 | `1` address | LCDC bit 4 sampled **again**; Y re-read |
| 6 | `1` read | bitplane 1 read; the window's tile counter advances here |
| 7+ | push | eight pixels enter the background FIFO, but only once it is empty |

`fifo.rs` implements exactly this: `B_A`, `B_R`, `LO_A`, `LO_R`, `HI_A`, `HI_R`, `PUSH`, one per
dot, with `fetcher_dot` doing the address work on the `_A` steps and the VRAM read on the `_R`
steps. The push is attempted on the same dot as the last read.

Two consequences matter, and most of the failing pictures were one of them:

- **A register sampled on an address dot takes effect one dot later than a register used by the
  read.** This is what the `*2` variants probe. They place pairs of sprites at crafted X positions
  so the mid-line write falls on a known dot of the fetch (the source tables insert 12 to 19
  stall dots), then show which dot the change first reaches the screen on.
- **LCDC bit 4 and Y are sampled twice**, once per bitplane. A write in between mixes two
  different tiles, or two different rows of one tile, into a single row of pixels. The Mealybug
  documentation says Y is read at `B`, `0` and `1` on a DMG and on CGB revisions up to CPU CGB C,
  and only at `B` on CGB CPU D and the AGB — so `m3_scy_change` is a model difference between
  revisions, not a bug to fix twice.

### The position counter, and the line's first eight pixels

The PPU counts its position on the line from **sixteen pixels before** the first one the LCD
takes, and the background FIFO starts the line holding **eight junk pixels**. Those junk pixels
are dropped one per dot, and on the dot the counter's low three bits match `SCX`'s the counter
jumps straight to minus eight. The pixels that leave the FIFO between that jump and position zero
are the fine scroll: that is the whole of `SCX & 7`, and it is why mode 3 is `172 + (SCX & 7)`
dots long rather than having a scroll term added to it. `fifo.rs` keeps that counter in `pos` and
does the jump in `render_pixel`; mode 3's length now falls out of the dot loop instead of being
worked out in advance, and it agrees with `ppu.rs::mode3_length` without being told to.

### How a register write reaches the drawer

This is the part that cost the most to get right, and the part a reader should not change without
re-measuring.

A real SM83 does not finish a register write at the same point of its M-cycle for every register,
and it does not write LCDC or a palette register in one go at all:

- a **palette** register (`BGP`, `OBP0`, `OBP1`) reads as **the old value OR the new one for a
  single dot**, and as the new value from the dot after;
- **LCDC** lands in two halves a dot apart: the first carries only bit 0, the background enable
  (and a clear of bit 1, objects, when the LCD is at the line's first pixel or an object is being
  fetched), and the second carries the whole value;
- **WX** finishes two dots after that group, at the very end of its M-cycle.

**The two bits the pixel output stage reads lag the bits the fetcher samples by one dot.** Those
two are bit 0 (background enable) and bit 1 (objects); the fetcher's are bits 3, 4 and 6 (the two
map bits and tile select). `registers_dot` gives bits 0 and 1 the old value for the single dot a
write lands on. This is the one change the measurement above pinned down exactly: before it, the
suppressed-pixel windows in `m3_lcdc_bg_en_change` were one pixel early in **all** of the
eighteen sprite positions the test sweeps and in both of each line's two windows - 376 pixels -
and after it 374 of those 376 pixels are right. Note the asymmetry the hardware's first half-write
implies, a set of bit 0 arriving a dot before a clear of it, is **not** what the reference
pictures show: a 12-dot gap between a clear and a set suppresses 12 pixels, not 11, so both edges
move together. The one-dot lag is the whole of it.

cBoy's bus does none of this: `Bus::write` lets the M-cycle's four dots pass and then applies the
write, so every write reaches the PPU at one fixed point, late. Two constants in `fifo.rs` carry
the correction, and **both were measured, not guessed**:

- `WRITE_BIAS = 3` runs the whole drawer three dots behind the dot counter. Of the candidate
  offsets 0 to 5, the DMG pictures passing are 5, 7, 5, **20**, 2, 2 — one sharp optimum.
- `WX_LAG = 2` gives the window its own, later view of WX. With `WRITE_BIAS` at 3 the lags 0 to 3
  give 16, 18, **20**, 19 pictures and 30829, 19187, **735**, 14545 differing pixels — the same
  two dots the hardware's write-conflict behaviour calls for.

`registers_dot` then reproduces the staging itself: the palette OR for one dot, the background-
and object-enable bits held back a dot, and the window-enable bit taken as the old AND the new so
that switching the window **off** reaches the PPU a dot before switching it **on** does. The map
and tile-select bits are deliberately left where cBoy's single-stage write puts them: holding
them back a dot as well breaks `m3_lcdc_bg_map_change`, `m3_lcdc_tile_sel_change`,
`m3_lcdc_tile_sel_win_change`, `m3_lcdc_win_map_change` and `m3_lcdc_obj_size_change` on exactly
the lines where those tests place a sprite at X 2 to 5. That is now explained rather than
suspicious: those bits are read by the fetcher, which is the half of the PPU that does **not**
lag, so a lag there is simply wrong.

### Clearing the object-enable bit aborts an object fetch

On a DMG, clearing LCDC bit 1 while an object is being fetched does not merely hide the object:
it **aborts the fetch**. The object is never mixed into the object FIFO, and the dots the rest of
the fetch would have taken are not spent, so mode 3 ends earlier and every pixel after it on that
line moves. The object stays queued and is dropped when the LCD passes its X, because with bit 1
clear no new fetch can start.

`registers_dot` does this on the dot the staged clear of bit 1 appears, which is the same dot the
hardware's first half-write carries it on, and the fetch counts as "in progress" from the dot the
object's X matched to the dot before its last read - not on that last read itself, where the
object's row has already been read. That is what `m3_lcdc_obj_en_change_variant` probes: it
disables objects mid-line with sprites at eighteen different X positions and then moves `BGP`
black and white again later in the line, so the picture shows where mode 3 got to. With the abort
in, it passes.

### The window

`fifo.rs` has one window-start rule with three cases, and no special cases beyond them. The whole
stack of patches the earlier sessions left (`win_stop`, `win_lock`, `win_hold`, `win_late`,
`win_blank`, `wx_zero`, `wx_armed`) is gone.

- `WY` is compared once per line; the match is sticky for the rest of the frame.
- With the match in hand and LCDC bit 5 set, the window starts on the dot where `WX` equals the
  PPU's position plus 7. Starting it clears the background FIFO and restarts the fetcher, which
  costs the six dots of a fresh tile fetch: that cost is what `m3_window_timing` draws as a
  staircase.
- **The window's line counter advances every time the window activates, not once per line.** Turn
  the window off and on again within one line and the second activation draws the *next* window
  row on the same screen line. `m3_lcdc_win_en_change_multiple` is precisely this.
- LCDC bit 5 is sampled at the `B` address dot, and that single rule is the whole of the
  disable behaviour: the window keeps drawing to the end of the tile in progress, and the
  background resumes at the next tile boundary with the low three bits of SCX having no further
  effect. So the window always covers a multiple of eight pixels unless it began off the left edge.
- Re-setting LCDC bit 5 in the same line does nothing unless `WX` has also been moved to a pixel
  that has not been drawn yet.
- `WX` = 0 with a non-zero SCX fine scroll starts the window one dot later. `WX` = 166 is an edge
  case; `WX` = 167 never triggers at all, because it would need a position of 161.
- On a DMG, when the `WY` match holds but LCDC bit 5 is clear and `WX` equals the current position
  plus 7, a single colour-0 pixel is inserted into the FIFO instead. This is the "window
  reactivation zero pixel" the `m3_wx_*_change` descriptions name.

### Objects

An object is fetched when its X matches the PPU position plus 8. The fetch does not begin
immediately: the background fetcher first runs on until it has reached its last read dot *and* the
background FIFO is non-empty, so an object landing early in a tile costs more dots than one
landing late. The object fetch itself takes six dots, during which the background fetcher is
stalled and the LCD is handed nothing; an object still waiting at X 0 holds up even the line's
fine-scroll discard. **Mode 3's length is the sum of those stalls.** `fifo.rs` now works that way
and its own dot count agrees with `ppu.rs::mode3_length`, which stays as the authority for STAT
and HBlank so that the two drawers cannot differ in anything the CPU can see.

The object's row address is worked out twice, once per byte, so LCDC bit 2 changing the object's
height between the two mixes two rows. Objects are kept reverse sorted by X, and among objects
sharing an X the lower OAM index is fetched first and so wins on a DMG; on the Color the OAM index
is carried into the FIFO and the lower index wins wherever they overlap.

## The 32 failures, grouped

Counts are differing pixels as the report prints them.

### Group 1 — blocked outside this drawer: CGB DMG-compatibility colour (31 pictures)

**Every one of the 31 CGB pictures is held back by this, before any timing question.** The
Mealybug ROMs are DMG cartridges; the CGB references were captured from a DMG cartridge running on
a CGB, where the boot ROM picks a colour palette from a hash of the cartridge title and the game
keeps using `BGP`/`OBP0`/`OBP1`. Measured from the reference pictures, they use exactly one
palette set: background `FFFFFF` / `7BFF31` / `0063C6` / `000000`, objects `FFFFFF` / `FF8484` /
`943939` / `000000`.

cBoy has no DMG-compatibility mode. `gb.rs` starts a CGB with every palette entry white
(`bg_pal = vec![0xFF, 0x7F].repeat(32)`), and both drawers take the CGB path, which indexes
`bg_pal` by the tile attribute and ignores `BGP` entirely. So a DMG ROM forced into CGB mode draws
a white screen, which is why these failures are enormous and nearly all report `got FFFFFF`:
`cgb:m3_window_timing_wx_0` 21942, `cgb:m3_window_timing` 21687, `cgb:m3_bgp_change_sprites`
20340, `cgb:m3_lcdc_win_en_change_multiple` 18990, `cgb:m3_scy_change` 18188,
`cgb:m3_scx_high_5_bits` 17419, `cgb:m3_bgp_change` 15936, `cgb:m3_lcdc_bg_en_change` 15894, and
so on down to `cgb:m3_lcdc_obj_en_change` at 206.

**What is needed, and where.** Two halves, and only the second is in a file this work owns:

1. **In `gb.rs` (not owned by this work).** When a CGB is given a cartridge whose header byte
   `$143` does not have bit 7 set, enter DMG-compatibility mode: load the boot ROM's colourisation
   palette into `bg_pal` and `obj_pal` and record that the machine is in compatibility mode. For
   these test ROMs the palette above is the one required. A real boot ROM derives it from the
   title hash; a table of the boot ROM's palette sets, keyed by that hash, is the honest
   implementation.
2. **In the drawers (owned).** While in compatibility mode, `BGP` must index background palette 0
   and `OBP0`/`OBP1` must index object palettes 0 and 1, exactly as on a DMG, instead of the tile
   attribute selecting the palette. Without step 1 this changes a white screen into a white
   screen, so it cannot be done or tested first.

Until step 1 exists, **the CGB half of Mealybug cannot move**, and chasing it from `fifo.rs` would
be guessing. The honest ceiling for this work is therefore the 24 DMG pictures, of which 23 pass.

### Group 1a — four CGB references that can never reach zero

`cgb:m3_wx_4_change`, `cgb:m3_wx_5_change`, `cgb:m3_wx_6_change` and
`cgb:m3_lcdc_win_en_change_multiple_wx` each contain **1146 pixels in 169 colours that a CGB
cannot output**: their channel values are not on the 5-bit ladder `(v << 3) | (v >> 2)` that every
real CGB colour sits on. The reference pictures were evidently captured through some blending.
All four currently report 1211 differing pixels, so they are already within 65 pixels of a floor
of 1146. **They must never be counted as achievable, and no change should ever be made to chase
them.** Note the DMG references for the same four tests are clean four-grey pictures, and all
four of those DMG pictures pass.

### Group 2 — the one DMG picture still failing, and exactly what it is

`dmg:m3_lcdc_bg_en_change`, **4 pixels on 4 lines**, all of them x=0, on lines 19 to 22. Those are
four of the eight lines where the test places its sprite at X=2; the other four have colour 0
there anyway, so they carry no evidence either way.

The measurement is worth writing down in full, because it rules out the obvious answers and the
next session should not pay for it twice. The test's handler writes LCDC four times a line -
clear bit 0, set, clear, set - 12, 8 and 8 dots apart, so each line has two windows of suppressed
background. Measured against the neutralised baseline, for **every one of the eighteen sprite
positions**:

- the second window is 8 pixels wide and starts and ends on exactly the pixels this drawer picks;
- the first window is 12 pixels wide and starts and ends on exactly the pixels this drawer picks,
  including the positions where the sprite's own fetch stalls the LCD inside the window and the
  window comes out 10 or 11 pixels wide instead;
- for sprite X=2 **only**, the reference suppresses one further pixel, at x=0.

So this is not a write-timing error: a 12-dot gap cannot suppress 13 pixels, and the pixel stream
cannot produce 29 pixels in the 28 dots from the first clear to the last set. It is one extra
blank pixel at the line's first pixel, in the one case where an object's fetch falls at position
-6, i.e. two pixels into the line's eight-pixel discard. It is not a general left-edge effect
either: `m3_lcdc_tile_sel_change` and `m3_lcdc_bg_map_change` both place a sprite at X=2 on the
same lines and both draw ink at x=0, pixel for pixel, in the reference and here.

Neither this drawer's model nor the model `Core/display.c` describes produces that pixel: in both,
an object matching at position -6 waits three dots for the fetcher to reach its last read, then
takes six, and the pixel stream resumes unbroken. Whatever the DMG does there is a left-edge
detail of the object fetch that is not in the material to hand, so it is recorded and left: one
pixel on four lines, chased no further. **Do not go looking for it by trying values.**

### Group 3 — what the register-write work fixed

`dmg:m3_lcdc_obj_en_change_variant` went from 96 differing pixels to pixel-for-pixel with the
object-fetch abort above, and `dmg:m3_lcdc_bg_en_change` from 376 to 4 with the output-stage lag.
Nothing else moved in either direction.

### Group 3a — what the restructure fixed

For the record, so a later session can tell a regression from a known gap. These seventeen DMG
pictures went from failing to pixel-for-pixel in this item: `m3_bgp_change_sprites`,
`m3_lcdc_bg_map_change`, `m3_lcdc_obj_en_change`, `m3_lcdc_obj_size_change`,
`m3_lcdc_obj_size_change_scx`, `m3_lcdc_tile_sel_change`, `m3_lcdc_tile_sel_win_change`,
`m3_lcdc_win_en_change_multiple`, `m3_lcdc_win_map_change`, `m3_obp0_change`,
`m3_scx_high_5_bits`, `m3_scy_change`, `m3_window_timing`, `m3_window_timing_wx_0`,
`m3_wx_4_change`, `m3_wx_5_change`, `m3_wx_6_change`. The five that already passed still do.

### Group 4 — not probed by any failing picture

No DMG picture fails on VRAM/OAM access blocking (item C6): Mooneye covers those edges from the
CPU side and passes 86/86 on both drawers. `m2_win_en_toggle`, the one `m2_*` picture, passes.

## What is left, in order

1. **CGB DMG-compatibility colour, in `gb.rs`** (not owned here). 31 pictures, none reachable
   before it, and 4 of them not reachable at all. This is now by far the largest item, and it is
   worth doing for players as much as for the tests.
2. **`Bus::write` applying an IO write at the hardware's point in the M-cycle** (not owned here;
   see below). That would let `WRITE_BIAS` and `WX_LAG` go to zero and the staging be written
   purely as the hardware's, with no correction constants at all. It would not move the score.
3. **The last 4 pixels of `m3_lcdc_bg_en_change`**, which Group 2 above characterises exactly and
   which nothing in the material to hand explains. Worth one look by someone who has a DMG and a
   logic analyser; not worth another session's guessing.

## Needed in files this work does not own

Collected for whoever owns them; nothing here has been changed by this work.

- **`gb.rs`: CGB DMG-compatibility mode.** As set out in Group 1. Without it, 31 of the 55
  Mealybug pictures cannot move, and cBoy shows a white screen for any DMG game a player runs in
  Colour mode. This is worth doing for players, not only for the tests.
- **`bus.rs`: where an IO write lands inside its M-cycle.** `Bus::write` calls `tick()` first and
  applies the write after, so a register write reaches the PPU three dots later than on hardware,
  and a register the hardware writes in two stages a dot apart arrives in one. `fifo.rs` corrects
  for it with `WRITE_BIAS` and `WX_LAG` and reproduces the staging itself, which is enough for 23
  of the 24 DMG pictures but cannot express a bit that arrives *before* the write cBoy is told
  about. The proper fix is for a write to `$FF40`-`$FF4B` to be applied part-way through its
  M-cycle rather than at the end of it. Nothing outside this drawer depends on the present
  behaviour, and Mooneye's 86 acceptance tests would have to stay at 86 either way.
- **`ppu.rs::mode3_length` is called from `tick` before the drawer runs**, so STAT and HBlank
  timing do not depend on which drawer is chosen. That is deliberate and must stay: Mooneye checks
  it from the CPU side on both drawers. The Exact drawer's own dot count now agrees with it.

## For the owner

Nothing has changed in how the app looks or plays, and the exact drawer is still switched off by
default.

The exact drawer has been rebuilt from scratch to work the way the real Game Boy chip does, step
by step, instead of the pile of patches it was. On the test set that measures this exactly, it
went from **5 pictures right out of 55 to 23**. Twenty-three is also all but one of the 24 tests
that are *possible* to pass today, and the last one is four pixels away: the other 31 are held up
by a separate problem (cBoy does not yet colour an old black-and-white game the way a Game Boy
Color does, so those come out white in Colour mode), which is written down for whoever fixes that
file.

**It is still not worth switching on for playing games.** No real game has been tried on it on a
phone, so Standard stays the one to use. The next session can pick this up from the notes above
without rediscovering anything.

If you want to look at Exact anyway, on the phone: **Settings → Core settings → cBoy → Picture
drawing → Exact (experimental)**, then open a game. Switch it back to **Standard** to return to
the drawer every game here has been tested on.
