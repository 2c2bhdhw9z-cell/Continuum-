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
| dmg-acid2 on a Color (its Color reference) | pass | pass |
| cgb-acid2 | pass | pass |
| Mooneye DMG acceptance | 87 / 87 | 87 / 87 |
| Mooneye Color | 7 / 7 runs | 7 / 7 runs |
| Mealybug Tearoom | not applicable | **50 / 55** |

**23 of the 24 DMG pictures**, and 27 of the 31 Color ones: every Color picture a Color can produce. The DMG picture that still fails is
`m3_lcdc_bg_en_change`, and it is **four pixels** away: one pixel on each of four lines, all of
them x=0, all of them on the eight lines where that test places a sprite at X=2. The Color half
went from 1 to 4 when a Color drew a black-and-white game in the colours its start-up code picks
(`compat.rs`, evening of 13 October), and from 4 to 21 when each picture-register write started
landing on the dot the Color lands it (`bus.rs::landing`, 16 October; "How a register write
reaches the drawer" below), and to 22 when the drawer did what a Color's hardware does in Game
Boy mode too, to 26 with the Color's whole tile-select glitch, and to 27 when an object's first
byte took its height a dot before its read ("The Color half, mapped" below, which has every Color
picture by name). The four Color pictures left can never reach zero at all (Group 1a).

`tests/roms.rs::MEALYBUG_PASSING` pins all 50 by name: a change that drops one of them is wrong,
whatever else it fixes.

The Mooneye gate reads 87 rather than the 86 this file quotes further down. Nothing in this work
moved it: another agent's link-cable work gave the serial port a real clock and added a ROM to the
gate. Both drawers pass all 87, which is the thing that matters here — the two drawers must never
differ in anything the CPU can see.

Several rules are in that no Mealybug picture can probe, so the score does not move for them and
they are pinned by unit tests in `ppu.rs` instead. The window's last two (the `WY` comparison
needing the window switched on, and `WX` = 166):
`the_window_waits_for_the_next_frame_when_it_is_switched_on_late`,
`switching_the_window_on_during_its_own_line_catches_the_match`,
`wx_166_leaves_the_window_running_into_the_next_line` and `wx_167_never_starts_the_window`. The
object stalls and the length of mode 3 that comes out of them:
`the_object_stalls_are_mode_3s_length`,
`an_object_at_x_0_waits_a_whole_tile_fetch_whatever_scx_says` and
`a_colour_pays_for_an_object_it_has_been_told_not_to_draw`. Line 144's STAT interrupt, which is
Mooneye's to measure rather than Mealybug's:
`line_144s_mode_2_interrupt_beats_vblank_only_on_a_colour` and
`a_colour_gets_no_extra_mode_2_interrupt_at_line_144`. Every one of them was checked to fail when
the rule it covers is taken back out, so they are evidence and not decoration.

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

A fourth kind of measurement settled Group 1, and it needs no emulator at all: compare two
**reference** pictures to each other. Map each one back to shade indices 0-3 through the palette
its console used — the Game Boy's four greys, or the compatibility palette below — and the answer
says which half of a difference you are looking at. Same shade index everywhere means the two
consoles drew the same picture and only the colour differs, which is a palette matter; a different
shade index means the hardware really did draw something else, which is a timing matter. Counting
the distinct colours in a reference is worth doing first for the same reason: a reference that
contains only black and white cannot be failing on palette at all. Both were done with a throwaway
PNG reader outside the repository; neither needs to live in the tests.

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
re-measuring. Since 16 October it lives where it belongs, in the bus, with no correction
constants left in the drawer.

A real SM83 does not finish a register write at the same point of its M-cycle for every register,
and it writes LCDC and the Game Boy's palette registers in two steps. SameBoy models this as a
table of "write conflicts" (`Core/sm83_cpu.c`, `cycle_write`, read and not copied):
for each register, on how many dots of the M-cycle the write lands, and what the PPU sees on
each step. `bus.rs::landing` is that table, in whole dots (4 is the end of the M-cycle, 5 one dot
into the next), and `Bus::write` hands the steps to the PPU on their dots (`Ppu::land`), while
the register itself still changes at the end of the M-cycle exactly as before. Only the exact
drawer reads what landed (`Ppu::seen`); everything the CPU can see (STAT, LY, mode 3's length,
the interrupts) and the whole of the Standard drawer stay timed on the register, so they cannot
have moved: a fingerprint of every frame, the CPU's registers and the save state, over 200 frames
of every test ROM and five Pokémon games on both consoles (336 runs), is identical before and
after.

| Register | Game Boy | Color (CPU CGB C, single speed) |
|---|---|---|
| `LCDC` | two steps, dots 2 and 3 (the first is below) | dot 4; clearing tile select glitches the next dot's bitplane read |
| `SCY` | dot 2 | dot 4 |
| `SCX` | dot 2 | dot 4 |
| `BGP`, `OBP0`, `OBP1` | old OR new on dot 2, new on dot 3 | new on dot 3 |
| `WX` | dot 4, and on the next dot the window's one-pixel-late start is not taken | dot 5 (the CPU's write wins the PPU's read of it, one dot into the next M-cycle) |
| `WY`, `STAT`, `LYC` | dot 4 | dot 4 |

In double speed SameBoy has a separate table in half dots, which a whole-dot drawer cannot place,
so every write still lands at the end of the M-cycle there. No Mealybug test runs in double speed.
The Super Game Boy and the Game Boy Pocket use the Game Boy's table; SameBoy gives the Super Game
Boy a simpler one of its own and marks it unverified, and nothing here can measure either.

`fifo.rs::DELAY` is the one number left on the drawer's side: mode 3 shows in STAT, and the
line's first fetch starts on the sixth dot after. It is SameBoy's, and **measured, not taken on
trust**: `scripts/sameboy/compare-landings.py` runs every Mealybug ROM on SameBoy (through
`scripts/sameboy/harness.c`) and on cBoy (cargo feature `trace`) and lines up the drawer's
position, LCD x, fetcher step, object fetch, window and FIFO length on the dot each write lands.
On the Game Boy set, 18,528 writes land in mode 3. All 6,696 `SCX`, `BGP`, `OBP0` and `WX`
writes find the two drawers in the same state, and so do 8,620 of the 8,640 `LCDC` writes; the
20 that do not are all in `m3_lcdc_win_en_change_multiple_wx`, a picture cBoy passes and SameBoy
misses by 43 pixels. The 3,192 `SCY` writes land a dot apart on purpose (below).

Two rows of the Game Boy's table are not SameBoy's, and both are measured against the Game Boy
pictures, which SameBoy's own model misses on nine (it passes 15 of the 24):

- **`SCY` lands on dot 2, with `SCX`**, where SameBoy has it a dot later. The Mealybug PPU
  documentation says the same thing from the other side: a Game Boy takes `SCY` at once, and a
  Color two dots later, which is dot 4 - where SameBoy and this table both have the Color. Put
  into a copy of SameBoy, this one change makes SameBoy pass `m3_scy_change` too (16 of 24).
- **The first `LCDC` step carries the bits the fetcher samples (tile map, tile select, object
  size, window map) already new**, while the two the pixel output stage reads - background enable
  and objects - are still old, and the window enable is the old AND the new, so switching the
  window off arrives a dot before switching it on. SameBoy's first step is the old value with only
  the new background enable OR'd in. Put into the same copy of SameBoy (keeping the LCD switch
  out of the first step, as cBoy's register does), this makes SameBoy pass six more Game Boy
  pictures - `m3_lcdc_bg_map_change`, `m3_lcdc_obj_size_change`, `m3_lcdc_obj_size_change_scx`,
  `m3_lcdc_tile_sel_change`, `m3_lcdc_tile_sel_win_change` and `m3_lcdc_win_map_change` - and
  leaves it four pixels off `m3_lcdc_bg_en_change` at exactly the four pixels cBoy misses
  (22 of 24). Both experiments were run in a scratch copy outside the repository.

Both steps on the Game Boy also clear the object enable early when the write clears it while
the LCD is at the line's first pixel or an object is being fetched, which is SameBoy's.

This is what the score moved on. The Color half had been landing every write with the Game Boy's
timing, because the drawer picked its staging by the cartridge's mode rather than by the console;
with the Color's own row, and nothing else changed, **17 Color pictures passed at once** (Mealybug
27 → 44 of 55), and every Game Boy picture that passed before still does.

### Clearing the object-enable bit aborts an object fetch

On a DMG, clearing LCDC bit 1 while an object is being fetched does not merely hide the object:
it **aborts the fetch**. The object is never mixed into the object FIFO, and the dots the rest of
the fetch would have taken are not spent, so mode 3 ends earlier and every pixel after it on that
line moves. The object stays queued and is dropped when the LCD passes its X, because with bit 1
clear no new fetch can start.

`Ppu::land` does this on whichever step of the write clears bit 1 as the drawer sees it, as
SameBoy's `GB_write_memory` does on whichever of its two writes clears it. The fetch counts as in
progress from the dot the object's X matched until its last read has happened, which is
SameBoy's `during_object_fetch` and is what lines the landings above up; the earlier version
stopped a dot sooner, and no picture can tell the two apart. That is what
`m3_lcdc_obj_en_change_variant` probes: it disables objects mid-line with sprites at eighteen
different X positions and then moves `BGP` black and white again later in the line, so the
picture shows where mode 3 got to. With the abort in, it passes.

### The window

`fifo.rs` has one window-start rule with three cases, and no special cases beyond them. The whole
stack of patches the earlier sessions left (`win_stop`, `win_lock`, `win_hold`, `win_late`,
`win_blank`, `wx_zero`, `wx_armed`) is gone.

- **`WY` is compared while the window is switched on, and the match is sticky for the rest of the
  frame.** The comparison runs at the start of mode 2 and again on every write to `WY` or `LCDC`,
  and it only makes the match while LCDC bit 5 is set at that moment, so a game that switches the
  window on *after* the line `WY` names has gone by gets no window until the next frame. This one
  is in `ppu.rs::wy_check`, which both drawers read, and it is the only piece of this work the
  Standard drawer shares. Running it on every dot is the same rule written shorter: neither
  register can change without a write, and `LY` only changes at the line boundary. One known
  approximation: on hardware a write-triggered comparison waits for the next eight-dot boundary
  of the fetch and the window cannot start on the dot it lands on, so cBoy can make the match up
  to eight dots early when a game switches the window on *during* the line `WY` names. No picture
  probes that - the ones that write LCDC mid-line (`m2_win_en_toggle` writes it in mode 2, the
  `m3_lcdc_win_*` family after the match is already made) all still pass - so the alignment was
  left out rather than invented.
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
- `WX` = 0 with a non-zero SCX fine scroll starts the window one dot later. `WX` = 167 never
  triggers at all, because it would need a position of 161.
- **`WX` = 166 on a DMG starts the window in HBlank, so it lands on the *next* line.** The start
  test asks whether the window begins on the *next* pixel, so 166 needs a position of 160 - which
  only comes round after mode 3 has handed the LCD its last pixel. No window pixel fits on the
  line itself; instead the window is left running when the line ends, and the next line is window
  from its first pixel, starting at the window's **second** tile (the first counts as fetched in
  that HBlank) and on a further window row. The row counter also advances when the position
  reaches 159 on the line that sets this off, so that line costs two rows and each line after it
  one. On the Color `WX` = 166 is an ordinary start, as `166 + cgb` in the rule says.
  `fifo.rs` carries this over in `win_carry`, set in `fifo_finish` and consumed by `fifo_start`.
- On a DMG, when the `WY` match holds but LCDC bit 5 is clear and `WX` equals the current position
  plus 7, a single colour-0 pixel is inserted into the FIFO instead. This is the "window
  reactivation zero pixel" the `m3_wx_*_change` descriptions name.

### Objects

An object is fetched when its X matches the PPU position plus 8. The fetch does not begin
immediately: the background fetcher first runs on until it has reached its last read dot *and* the
background FIFO is non-empty, so an object landing early in a tile costs more dots than one
landing late. The object fetch itself takes six dots, during which the background fetcher is
stalled and the LCD is handed nothing - the fetcher does take one further step on each of the
first two of those six - and an object still waiting at X 0 holds up even the line's fine-scroll
discard. **Mode 3's length is the sum of those stalls.** `fifo.rs` works that way, and
`ppu.rs::mode3_length` stays the authority for STAT and HBlank so the two drawers cannot differ in
anything the CPU can see.

The object's row address is worked out twice, once per byte, so LCDC bit 2 changing the object's
height between the two mixes two rows. Objects are kept reverse sorted by X, and among objects
sharing an X the lower OAM index is fetched first and so wins on a DMG; on the Color the OAM index
is carried into the FIFO and the lower index wins wherever they overlap.

#### The two stalls `mode3_length` was not charging for

The drawer's dot count and `mode3_length` were measured against each other for every sprite
layout, and they disagreed in two places. Both are about what a stall *costs* rather than what it
draws, so no Mealybug picture can see either: they are pinned by
`ppu.rs::the_object_stalls_are_mode_3s_length`, which compares the dot the drawer hands over its
last pixel on against the dot `mode3_length` puts mode 0 at, for one object at every X, for pairs
that share a tile and pairs that do not, and for the ten-object limit at every spacing - with the
objects inserted in the reverse of X order so the OAM order is not the fetch order - across SCX
0-7, on a DMG, on a Color, on a Color with objects switched off, and with 8x16 objects. The
drawer counts single dots and STAT shows whole M-cycles; that rounding is now the only difference
left, in all of them.

- **An object at X=0 waits a whole tile fetch, whatever SCX's low bits say.** Rendering, *and the
  fine-scroll adjustment with it*, does not happen while an object at X=0 is pending, so such an
  object is always fetched with the fetcher still on the first step of its tile: five dots of
  waiting and six of fetching, eleven in all. `mode3_length` was shortening that wait by the fine
  scroll, which is the one case where the two drawers' idea of mode 3 differed - by up to five
  dots, whenever a line had a sprite at X=0 and a non-zero `SCX & 7`. With the rule in, the
  formula reproduces the drawer's dot count **exactly** (not merely to the M-cycle) in all 2140
  layouts measured; without it, 129 of them are wrong.
- **A Color pays for an object even when it has been told not to draw one.** Clearing LCDC bit 1
  makes a DMG skip the object fetch entirely, but a Color runs the fetch anyway and only drops the
  pixel as it leaves the FIFO, so mode 3 is just as long as with objects on. `mode3_length` was
  returning the bare length for both, while the drawer - rightly - was spending the dots.

#### One thing from SameBoy that is deliberately *not* here

`Core/display.c` moves the LCD back a pixel (`lcd_x--`) when a DMG starts the window through the
"one pixel late" case, and calls it an LCD-PPU horizontal desync that happens on DMG units but not
all of them. cBoy has that window case but not the pixel it swallows, and that is correct for
these references: the `DMG-blob` pictures do not show the desync, and `m3_window_timing`,
`m3_wx_4_change`, `m3_wx_5_change`, `m3_wx_6_change` and `m3_lcdc_win_en_change_multiple_wx` all
pass pixel for pixel without it. Adding it would break five passing pictures.

### What the CPU can see while the picture is being drawn

Mode 2 and mode 3 shut the CPU out of OAM and VRAM, and the dots on which that starts and stops
are measured from the CPU side by twelve of Mooneye's 86 (`lcdon_timing`, `lcdon_write_timing`,
`intr_2_oam_ok_timing`, `intr_2_mode0_timing`, `intr_2_mode3_timing`, `intr_2_0_timing`,
`intr_1_2_timing`, `intr_2_mode0_timing_sprites`, `hblank_ly_scx_timing`, `stat_irq_blocking`,
`stat_lyc_onoff`, `vblank_stat_intr-GS`). Those pass on both drawers, and the four windows in
`ppu.rs` (`oam_blocked`, `vram_blocked`, `oam_write_blocked`, `vram_write_blocked`) were read
dot by dot against SameBoy's `Core/display.c` to see whether passing was luck. It was not: on a
Game Boy cBoy's four windows are SameBoy's model, edge for edge.

The mapping, for anyone doing this again: SameBoy's visible-line loop starts 4 dots before cBoy's
dot 0, because its line boundary sits where `LY` changes and cBoy's sits where STAT shows mode 2.
So SameBoy's `+4` is cBoy's dot 0, and its `oam_search_index == 37` is cBoy's dot 76 — which is
where cBoy's `vram_blocked` already has its `(76..80)` term, from the opposite direction.

What is **not** the same is the Color, and every difference is in the same place: SameBoy keys
five of these edges off `GB_is_cgb(gb)`, hardware rather than cartridge mode, which `ppu.rs` could
not see when this was written (see "A Color's line 144" below; the flag is live now). They are
written down rather than implemented because **nothing in either test suite probes any of them** —
Mooneye's Color set is boot, register and STAT ROMs, and the Mealybug Color pictures are stopped by
Group 1 long before these edges matter. That reason still stands with the flag wired: implementing
five unprobed edges blind is how the score moved up and down before, so whoever takes them needs a
way to measure first.

1. A Color keeps OAM **writes** blocked through the last 4 dots of mode 2 (cBoy dots 76-79), where
   a Game Boy lets them through. cBoy's `oam_write_blocked` ends mode 2's block at dot 76 for
   both.
2. A Color does **not** block VRAM reads in those same 4 dots, where a Game Boy does. cBoy's
   `vram_blocked` has the `(76..80)` term for both.
3. A Color starts blocking OAM writes a whole M-cycle before the line begins (cBoy dot 452 of the
   line before, and unconditionally from dot 454), where cBoy starts at dot 0 for both.
4. In double speed a Color revision earlier than CGB-D does not block OAM reads at the line
   boundary at all. That one needs the PPU to know about double speed as well, which it also
   cannot see today.
5. A Color of revision CGB-D or later blocks OAM reads for the first dot of mode 0. cBoy never
   does.

### A Color's line 144

On every line the mode 2 STAT interrupt is raised one M-cycle *before* mode 2 shows in STAT — so
in the M-cycle before the line it belongs to begins, which is `dot >= 452` of the line before
(`ppu.rs::oam_irq_early`; SameBoy says the same in one comment, "the OAM STAT interrupt occurs 1
T-cycle before STAT actually changes").

Line 144 has no mode 2, and its interrupt is the one place the two consoles disagree:

- A **Color** raises it one M-cycle early like every other line's, so it arrives **one M-cycle
  before VBlank**.
- A **Game Boy** holds that one back to the start of line 144, where it lands **together with
  VBlank**.

This is not inferred from pictures. It is what Mooneye's own pair of tests say, and they are the
specification: `acceptance/ppu/vblank_stat_intr-GS` and `misc/ppu/vblank_stat_intr-C` are the same
program, both timing the interrupt with `DIV` against a run of `nop`s, and the Color one uses
**one `nop` fewer**. Their headers record which real consoles each was verified on: GS passes on
DMG, MGB, SGB and SGB2 and fails on CGB, AGB and AGS, and C the other way round.

`stat_condition`'s `ly == 144 && dot < 4` term is the Game Boy's late raise. It is left in for
both consoles, which costs the Color nothing: the early raise has already put the line high, so
there is no second rising edge, and that matches SameBoy, which also asks twice and guards the
second ask with `!gb->stat_interrupt_line`.

Pinned by `line_144s_mode_2_interrupt_beats_vblank_only_on_a_colour`, which runs the PPU in whole
M-cycles exactly as `Bus::tick` does and pins the numbers (113/113 on a Game Boy, 112/113 on a
Color — which also pins the alignment the rule rests on, a 456-dot line run four dots at a time
putting dot 452 at the end of M-cycle 112 and the next line's dot 0 at the end of 113), and by
`a_colour_gets_no_extra_mode_2_interrupt_at_line_144`, which pins that the Color's raise is line
144's own interrupt moved and not a second one. The first fails when the rule is taken out; the
second fails against the obvious wrong version of it, one that lets the line fall again between
dot 452 and the start of line 144.

**The rule is in, and it is now switched on**: it was written against `ppu.cgb_hardware` while
nothing set that flag, and another agent wired it up in `gb.rs` later the same night, which took
Mooneye's Color set to 7 of 7 runs with `vblank_stat_intr-C` in the gate. Anything else in this
file that says the flag is dead is out of date; the live flag is what makes the six Color pictures
in Group 1 step 2 reachable.

## The Color half, mapped (16 October)

Thirty-one of the 55 pictures were photographed on a Color (CPU CGB C) playing these
black-and-white cartridges. With the Color's write landing in (above), every one of the 31 was
run, compared with the reference **and with SameBoy's own picture of the same ROM**
(`scripts/sameboy/compare-pictures.py`; SameBoy itself gets 21 of the 31), and grouped by the
hardware behaviour it probes.

**27 pass.** `m2_win_en_toggle`, `m3_bgp_change`, `m3_bgp_change_sprites`,
`m3_lcdc_bg_en_change`, `m3_lcdc_bg_en_change2`, `m3_lcdc_bg_map_change`,
`m3_lcdc_bg_map_change2`, `m3_lcdc_obj_en_change`, `m3_lcdc_obj_en_change_variant`,
`m3_lcdc_obj_size_change`, `m3_lcdc_obj_size_change_scx`, `m3_lcdc_tile_sel_change`, `m3_lcdc_tile_sel_change2`,
`m3_lcdc_tile_sel_win_change`, `m3_lcdc_tile_sel_win_change2`, `m3_lcdc_win_en_change_multiple`,
`m3_lcdc_win_map_change`,
`m3_lcdc_win_map_change2`, `m3_obp0_change`, `m3_scx_high_5_bits`, `m3_scx_high_5_bits_change2`,
`m3_scx_low_3_bits`, `m3_scy_change`, `m3_scy_change2`, `m3_window_timing`,
`m3_window_timing_wx_0`, `m3_wx_4_change_sprites`.

How they got there, group by group, each group one commit:

1. **The compatibility palette** (13 October, `compat.rs`): 1 → 4.
2. **When each write lands, on a Color** (16 October, `bus.rs::landing`, G2): 4 → 21. The
   Color takes a palette write whole a dot before the end of the M-cycle with no old-OR-new dot,
   LCDC, SCX and SCY at the end, and WX a dot into the next M-cycle. Before this the drawer chose
   its staging by the cartridge's mode, so a Color running a Game Boy cartridge landed every
   write like a Game Boy. This group alone fixed the palette pictures, the LCDC bit pictures, the
   window-timing pair (the "one pixel per line" the old map below describes was this) and SCY.
3. **What the Color's hardware does whichever mode it is in** (16 October, `fifo.rs`, G3): 21 →
   22. SameBoy keys each of these off the console (`GB_is_cgb`), and the drawer had been keying
   them off the cartridge's mode, so a Color in Game Boy mode behaved like a Game Boy. They are
   now `colour_console()`: the background fetch reading the position a dot early except during
   an object fetch (the one that fixed `m3_scx_high_5_bits_change2`), the tile attribute read
   with the tile number, the WX fetch glitch, objects fetched even with objects switched off
   (and the X 0 hold that goes with it), the window's start rules (WX 166 is an ordinary start,
   no one-pixel-late start, no extra dot for WX 0 with a fine scroll, no WX 166 carry into the
   next line), no colour-0 pixel for a window switched off at WX, and no object-fetch abort when
   objects are switched off. What stays on the mode, as in SameBoy: the palettes (`BGP` and the
   compatibility colours), background enable as a priority switch rather than a blank, the
   object palette number, VRAM bank 1 for objects, and object priority by OAM index.
   **One deliberate exception:** the colour-0 pixel the window hands the LCD when WX lands on it
   again while it is running. SameBoy leaves it out on every Color but for WX 0, and doing the
   same costs `m3_wx_4_change_sprites` exactly SameBoy's 10 pixels; the reference for CPU CGB C,
   the one for CPU CGB D, and the photo of the real CPU CGB C screen (`photos/CPU CGB C`, looked
   at) all show those pixels. So a Color in Game Boy mode keeps it; a Color in colour mode, which
   nothing photographs, keeps SameBoy's rule.
4. **The tile-select glitch on a Color** (16 October, `fifo.rs::bitplane_read`, G3): no picture
   yet, but 1,141 pixels fewer wrong. Clearing LCDC bit 4 on the very dot of a bitplane read
   makes a Color (CPU CGB C and earlier) read the tile number instead, for a tile in the half of
   tile memory only the set position reaches (the Mealybug PPU documentation; SameBoy's
   `data_for_tile_sel_glitch`). `m3_lcdc_tile_sel_change` went 520 → 8 pixels,
   `m3_lcdc_tile_sel_win_change` 260 → 8, `m3_lcdc_tile_sel_change2` 408 → 186 and
   `m3_lcdc_tile_sel_win_change2` 401 → 179: exactly SameBoy's counts, pixel for pixel.
5. **The rest of the tile-select glitch, from the hardware documentation** (16 October, G4):
   22 → 26. SameBoy stops there; the Mealybug PPU documentation, which is where SameBoy's half
   came from, says two more things, and both are in now. **Setting** tile select on the dot of a
   bitplane read glitches it too, on every Color revision (SameBoy does that only in double
   speed). And what a set hands the fetcher is the last byte the PPU read from the object tile
   area - an object's row, or a background tile's fetched with tile select set - which is the
   documentation's "bitplane 1 data from the most recently drawn sprite ... or the most recently
   drawn tile as when TILE_SEL was last reset"; SameBoy instead keeps every background read. With
   both, all four tile-select pictures pass. Measured, not tried until it fit: the trace showed
   the set landing on a read dot on exactly the lines that were wrong (lines 128 to 135 of
   `m3_lcdc_tile_sel_change`, sprite X 16), and the one wrong line, 128, is the only one of the
   eight where the last object-area read was the previous line's glitched read of the black tile
   rather than an object's row. Keeping every read instead, or no object reads, or only bitplane
   1 of a glitched read, leaves 0 to 3 of the four passing. The documentation's third source, the
   read cut off at the end of the previous line, no picture needs and is not modelled.
6. **When an object's height is read for each of its two bytes** (16 October, G4): 26 → 27.
   `m3_lcdc_obj_size_change_scx` switches 8x16 objects to 8x8 and back mid-line, and on a Color
   four lines of its flipped object came out wrong in SameBoy and here alike. Reading the tile
   memory settled what the reference shows: on those lines the object's first byte comes from
   the 8x16 row and its second from the 8x8 row, so the switch to 8x8 lands after the first
   byte's height was taken and before the second's. In cBoy the switch landed on the dot before
   the first byte's read, so the first byte's address has to be worked out a dot before its read,
   which is how a background bitplane's is (address on one dot, read on the next); the second
   byte's is worked out on its own read dot. All four ways of placing the two were run: only this
   one passes all four object-size pictures. Both on the read dot (SameBoy's) misses this one by
   SameBoy's 15 pixels; both a dot early misses `dmg:m3_lcdc_obj_size_change` and
   `dmg:m3_lcdc_obj_size_change_scx` by 15 and 30, which are exactly SameBoy's own Game Boy
   misses on them (SameBoy's later first LCDC step comes to the same thing there).

After the first four, cBoy's picture was **SameBoy's, pixel for pixel, on 27 of the 31**, and on
the other four (`m3_wx_4_change`, `m3_wx_5_change`, `m3_wx_6_change`, `m3_wx_4_change_sprites`) it
differed from SameBoy only by the photographed zero pixel above. The fifth then went past
SameBoy on the four tile-select pictures.

**4 do not pass, and all four are the references no Color can produce:**

- **Can never pass (4):** `m3_wx_4_change`, `m3_wx_5_change`, `m3_wx_6_change`,
  `m3_lcdc_win_en_change_multiple_wx`. Their references hold colours no Color can show (Group 1a
  below). Not to be chased.

## The 31 failures, grouped

Counts are differing pixels as the report prints them.

### Group 1 — the Color pictures: the harness was running the wrong console (30 pictures)

**Status, 16 October:** written before the write landing went in. Since then 21 of the 31 Color
pictures pass, including all six of step 2, the whole of step 3 but `m3_lcdc_tile_sel_change2`,
`m3_lcdc_tile_sel_win_change2` and `m3_scx_high_5_bits_change2`, and step 4 but
`m3_lcdc_obj_size_change_scx`, `m3_lcdc_tile_sel_change` and `m3_lcdc_tile_sel_win_change`.
What follows is kept for the measurements in it.

This group was misread until it was measured, so the finding comes first: **the Color references
were being compared against a machine they were never captured on.**

Every one of the 31 Mealybug ROMs is a **Game Boy cartridge** — header byte `$143` is `00` in all
of them, no test invokes the `cgb_mode` macro that would set it, and no test reads register `A` at
boot to ask which console it is. Mealybug's README says the folder a reference sits in names the
console it was photographed on, and that these tests deliberately give different results on a
Game Boy and a Color. So the `CPU CGB C` pictures are **a Game Boy cartridge in a Color console**,
which is that console's **Game Boy-compatibility mode**: the boot ROM picks a colour palette from
a hash of the cartridge title, and the game goes on using `BGP`/`OBP0`/`OBP1`. Measured from the
references, all of them use one palette set: background `FFFFFF` / `7BFF31` / `0063C6` / `000000`,
objects `FFFFFF` / `FF8484` / `943939` / `000000`.

The harness was instead building the machine with `GameBoy::new_mode(rom, true)`, which is full
**colour** mode — the same thing the core's own "Game Boy cartridge in Color mode" option does by
rewriting `$143`. In colour mode the drawer indexes `bg_pal` by the tile attribute and ignores
`BGP`, and `gb.rs` starts every palette entry white, so a Game Boy cartridge drew a **white
screen** and all 31 pictures failed enormously. (That is not a bug in colour mode: a real Color
console given a cartridge whose header claims colour does not colourise it either, and its boot
ROM leaves the palettes white. It is simply not this reference's machine.)

`tests/roms.rs` now builds the Color runs with `GameBoy::with_model(rom, Model::Cgb, None)`, which
for a Game Boy cartridge is exactly compatibility mode — `gb.rs` already implements it, and it is
what a player gets today when they pick the Color console for an old game. **That alone took
Mealybug from 23 to 24**, and it turned the other failures from "everything is white" into real,
small, readable differences: `m3_window_timing` 21687 → **138**, `m3_window_timing_wx_0` 21942 →
**144**, `m3_lcdc_obj_en_change` 206 → **60**, `m3_lcdc_win_map_change` 1102 → **182**,
`m3_lcdc_bg_map_change` 1362 → **384**.

The 30 that still fail now split four ways, each measured rather than guessed. The method was to
map both references back to **shade indices** through the palettes above: where the Color
reference and the DMG reference have the same shade index everywhere, the two consoles drew the
same picture and only the colour differs; where they do not, the Color hardware really did draw
something else.

1. **Needs nothing but the compatibility palette (3 pictures) — DONE, all three pass.**
   `cgb:m2_win_en_toggle`, `cgb:m3_lcdc_win_en_change_multiple` and `cgb:m3_wx_4_change_sprites`.
   Their references are the DMG picture shade for shade, and this drawer already draws all three
   of those DMG pictures pixel for pixel. Every differing pixel was a shade 1 or 2 that wants
   `7BFF31` or `FF8484` and got the Game Boy's `AAAAAA`. With the palette in, these three passed
   and nothing else moved either way: 24 to 27, exactly as predicted here.
2. **Black and white already, so no palette needed — but the Color drew a different picture
   (6 pictures).** `cgb:m3_lcdc_bg_map_change` 384, `cgb:m3_lcdc_obj_en_change_variant` 268,
   `cgb:m3_lcdc_win_map_change` 182, `cgb:m3_window_timing_wx_0` 144,
   `cgb:m3_window_timing` 138, `cgb:m3_lcdc_obj_en_change` 60. These references contain only
   `FFFFFF` and `000000`, which are the same two colours in the compatibility palette as in the
   Game Boy's greys, so colour cannot be what is wrong: what is left is Color **timing**. Note the
   shape of it — `m3_window_timing` differs by 138 pixels on 138 lines and `m3_window_timing_wx_0`
   by 144 on 144, i.e. **one pixel per line**, the window starting a pixel later on a Color than
   on a Game Boy. SameBoy has exactly one DMG-only pixel in its window start (the `lcd_x--`
   desync described above, which cBoy leaves out because the DMG references do not show it), and
   this is very likely its other side. **It is not implemented here, because when this was written
   it could not be measured:** the drawer only knows it is Color hardware through
   `ppu.cgb_hardware`, and at the time nothing set it, so writing the rule would have meant
   asserting an inference against a test written from the same inference — which is how the score
   moved up and down before. **Another agent wired that flag up in `gb.rs` later the same night,
   so this is now unblocked, and it is the best-value work left in this file:** these six have the
   clearest evidence of anything remaining, each one's gap is already counted to the pixel, and a
   drawer that knows which console it is can now be measured against the references directly.
3. **The seven `*2` variants.** `m3_lcdc_bg_en_change2`, `m3_lcdc_bg_map_change2`,
   `m3_lcdc_tile_sel_change2`, `m3_lcdc_tile_sel_win_change2`, `m3_lcdc_win_map_change2`,
   `m3_scx_high_5_bits_change2` and `m3_scy_change2` have a Color reference and no DMG one. They
   are also Game Boy cartridges, so they run in compatibility mode too, where the Color palette
   registers cannot be written — which is why their references show the boot palette's `FF8484`
   and not the colours the ROMs try to set. All seven need the palette, and most need Color timing
   as well.
4. **The remaining 10 need both the palette and Color timing.** `cgb:m3_bgp_change`,
   `cgb:m3_bgp_change_sprites`, `cgb:m3_lcdc_bg_en_change`, `cgb:m3_lcdc_obj_size_change`,
   `cgb:m3_lcdc_obj_size_change_scx`, `cgb:m3_lcdc_tile_sel_change`,
   `cgb:m3_lcdc_tile_sel_win_change`, `cgb:m3_obp0_change`, `cgb:m3_scx_high_5_bits` and
   `cgb:m3_scy_change`. (`m3_scy_change` is the known revision difference: `Y` is re-read at the
   `0` and `1` address dots on a Game Boy and on Color revisions up to CPU CGB C, so this one also
   needs the drawer to tell CGB-C from CGB-D.)

Steps 3 and 4 were written before the palette went in. With it in, every colour those seventeen
references use is one cBoy now draws, and their counts fell accordingly - `cgb:m3_bgp_change`
10864 → 496 pixels, `cgb:m3_scx_high_5_bits` 6421 → 65, `cgb:m3_lcdc_bg_en_change2` 2971 → 106.
What is left in them is where and when a Color draws those colours, which is the next thing to
map. The six in step 2 did not move, as their black-and-white references said they would not.

**Both halves of the palette are in (evening of 13 October).**

1. **The colours, in `gb.rs` and `compat.rs`.** A Color given a cartridge whose header byte `$143`
   does not have bit 7 set gets what its start-up code leaves: the palette set it picks from the
   title sum (and, for the fourteen shared sums, the fourth letter) when the game is Nintendo's,
   the default set otherwise — which is what every Mealybug ROM gets, their licensee being `00` —
   written into background palette 0 and object palettes 0 and 1, OPRI set, and B holding the
   title sum. The table is the documentation's (Pan Docs for the algorithm, The Cutting Room
   Floor for the data). One row of that page has its two numbers swapped (Radar Mission), and the
   start-up code's own byte settles it; `compat.rs` says so where the row is. Checked twice: once
   by a throwaway script outside the repository against the tables in SameBoy's start-up code,
   for all 256 sums and every printable fourth letter (no difference), and by running a real
   start-up file - SameBoy's free one, built from its source - through cBoy, which leaves the
   same palette memory, object order and registers as cBoy's own for every title the table knows,
   every shared sum with a letter it does not know, a game that is not Nintendo's, and all twelve
   button schemes, on a Color and on an Advance (284 starts,
   `tests/roms.rs::a_colour_start_up_file_gives_the_same_colours`, which needs the file, so it
   is run by hand).
2. **The drawers, `ppu.rs::mono_colour`.** In compatibility mode both drawers look the colour up
   in palette memory: the background in palette 0 and an object in palette 0 or 1 by its OBP bit,
   with the `BGP`/`OBP0`/`OBP1` shade as the colour number, which is SameBoy's
   `render_pixel_if_possible`. The shade and source buffers are filled as before, so the Game Boy
   palette and cBoy's own looks still go over the shades whenever the player has not asked for a
   Color's colours. dmg-acid2 on a Color now matches that test's own Color reference on both
   drawers.

The attribute byte stays 0 in compatibility mode in both drawers. SameBoy reads it from VRAM bank
1 on any Color, but the start-up code clears that bank and a game in Game Boy mode cannot reach
it, so the two agree; it is written here so nobody mistakes it for a difference.

### Group 1a — four CGB references that can never reach zero

`cgb:m3_wx_4_change`, `cgb:m3_wx_5_change`, `cgb:m3_wx_6_change` and
`cgb:m3_lcdc_win_en_change_multiple_wx` each contain **1146 pixels in 169 colours that a CGB
cannot output**: their channel values are not on the 5-bit ladder `(v << 3) | (v >> 2)` that every
real CGB colour sits on. The reference pictures were evidently captured through some blending.
**They must never be counted as achievable, and no change should ever be made to chase them.**
Note the DMG references for the same four tests are clean four-grey pictures, and all four of
those DMG pictures pass.

Their counts moved with the console fix in Group 1 and are no longer near the floor — 18648,
19811, 19701 and 23038 — because these four are now being drawn in the Game Boy's greys against
blended Color colours rather than in white against them. That is expected and means nothing: the
1146-pixel floor is a property of the reference pictures, not of the drawer.

### Group 2 — the one DMG picture still failing, and exactly what it is

`dmg:m3_lcdc_bg_en_change`, **4 pixels on 4 lines**, all of them x=0, on lines 19 to 22. Those are
four of the eight lines where the test places its sprite at X=2; the other four have colour 0
there anyway, so they carry no evidence either way.

Two things about it were settled by measurement afterwards. Neither makes the four pixels pass;
both rule out a wrong answer that looks obvious, and together they say the shape of what is
missing.

**It is not a difference between Game Boy revisions.** Mealybug ships a second Game Boy reference
for this very test, `expected/DMG-CPU B/m3_lcdc_bg_en_change.png` — one of only two pictures in
that folder, which is itself a hint that this test is where units disagree. The two Game Boy
references differ in **228 pixels**, and every one of them is the *right-hand* end of a
suppression window: `DMG-blob` suppresses one pixel more than `DMG-CPU B` at x=10 through x=15 and
x=26 through x=31, moving one pixel right for each group of eight lines as the test moves its
sprite. This drawer matches `DMG-blob` at all 228. At x=0 on lines 19 to 22, though, **both**
references are colour 0, so the extra blank pixel at the left edge is what both units do.

Read as windows, with the first suppression window of one of those lines written as first and last
suppressed pixel: `DMG-CPU B` has **[0, 11]**, twelve pixels from the handler's twelve-dot gap;
`DMG-blob` has **[0, 12]**, thirteen pixels from the same twelve dots; this drawer has
**[1, 12]**, twelve pixels one place to the right. Both units start at the line's first pixel and
this drawer starts at its second. **So the fix is not to move the window**: shifting it one dot
earlier would land on `DMG-CPU B`'s window exactly and give up all 228 pixels where this drawer
currently agrees with the reference the harness uses, trading four wrong pixels for 228. What is
missing has to *add* a suppressed pixel at the line's first pixel while leaving the window's end
where it is — thirteen pixels out of twelve dots — which is the one-pixel LCD/PPU desync SameBoy
describes for some DMG units and not others, and which cBoy deliberately does not model because
modelling it in the window breaks five passing pictures. `DMG-blob` looks like a unit that has it
and `DMG-CPU B` like one that does not.

**It is not an object's pixel either**, which was the other obvious suspect, since all four lines
are ones where the test places a sprite at X=2 and an object's left-hand pixels are clipped there.
`MEALY_DETAIL` now prints a `from` row saying which stage each pixel left the drawer through — 0
for the background, 1 or 2 for an object through `OBP0` or `OBP1` — and on lines 16 to 22 that row
is 0 across all 160 pixels: those lines draw no object pixel at all. (The row is not decoration:
56 lines of this same test do draw object pixels, and `m3_lcdc_obj_en_change` shows them at x=0.)
All four differing pixels are background the drawer failed to suppress.

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
takes six, and the pixel stream resumes unbroken. `fifo_overlay_object_row` lays all eight of an
object's pixels into the object FIFO at its head and lets the left-edge clipping fall out of the
discard, and SameBoy pops the object FIFO in lockstep with the background one through the whole
discard, exactly as `render_pixel` does — so for an object at X=2 both put the object's seventh
pixel at x=0 and both leave the background stream unbroken. Whatever the DMG does there is a
left-edge detail that is not in the material to hand, so it is recorded and left: one pixel on four
lines, chased no further. **Do not go looking for it by trying values**, and in particular do not
move the window start: the paragraph above measures what that costs.

Re-checked once more while the object model was being gone over against `Core/display.c` line by
line (the wait before a fetch, the six dots, the X=0 hold, the row address, the 8x16 masking, the
DMG's X-then-index order and the Color's index order): all of them agree, the left-edge pixel is
still unexplained, and nothing else about objects differs. The one SameBoy behaviour cBoy leaves
out on purpose is the window's LCD desync described above, which cannot reach this test because it
uses no window.

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
CPU side and passes 86/86 on both drawers. `m2_win_en_toggle`, the one `m2_*` picture, passes. The
four blocking windows were read against SameBoy anyway rather than trusted — see "What the CPU can
see while the picture is being drawn" above for the result, which is that the Game Boy side is
right edge for edge and all five differences left are the Color's.

## What is left, in order

1. ~~**The compatibility palette.**~~ **DONE, evening of 13 October:** `compat.rs`, `gb.rs` and
   both drawers (Group 1, "Both halves of the palette are in"). The three Color pictures it was
   predicted to pass passed, and the player-facing half is the "Colour for black-and-white games"
   setting.
2. **The six black-and-white Color pictures in Group 1 step 2.** The `gb.rs` flag they were
   waiting on was wired up by another agent during this same night, so these are the first real
   drawer work available to the next session, and they are the best-value item in this file: no
   palette is needed (their references are black and white only), the flag is live, and every
   one's gap is already measured to the pixel — two of them off by exactly one pixel per line,
   which is a window that starts a pixel later on a Color than on a Game Boy. Do this one first.
3. ~~**`Bus::write` applying an IO write at the hardware's point in the M-cycle.**~~ **DONE, 16
   October** (`bus.rs::landing`; "How a register write reaches the drawer" above). `WRITE_BIAS`
   and `WX_LAG` are gone, and so is the drawer's own staging. It did move the score after all:
   the Color had been landing writes on the Game Boy's dots, and with its own row 17 Color
   pictures passed.
4. **The five Color-only access-blocking edges** in "What the CPU can see while the picture is
   being drawn" above. They need the same `gb.rs` flag as item 1, and two of them need the PPU to
   know about double speed too. No test in either suite probes them, so whoever does this needs a
   way to measure first; the edges themselves are written down there in full.
5. **The last 4 pixels of `m3_lcdc_bg_en_change`**, which Group 2 above characterises exactly and
   which nothing in the material to hand explains. Looked at twice now, from the register-write
   side and from the object side, with the same answer both times. Worth one look by someone who
   has a DMG and a logic analyser; not worth another session's guessing.
6. **Mode 3's length on a Color in Game Boy mode with objects switched off.** The drawer now
   fetches the line's objects anyway, as a Color does (SameBoy), but `ppu.rs::mode3_length` still
   charges nothing for them unless the cartridge is in colour mode. That length is what STAT and
   HBlank run on for both drawers, so changing it would change the Standard drawer's timing and
   what a game can measure; no test measures it (Mooneye's Color set has no such ROM, and every
   Mealybug picture that switches objects off passes), so it was left and is written down here.
7. **Two window edges left alone on purpose**, both written down here so nobody has to rediscover
   them:
   - On the Color, a window started by `WX` = 166 also makes STAT report mode 0 for the rest of
     that dot, which can fire a spare HBlank interrupt. That is STAT timing rather than drawing,
     no Mealybug picture probes it, and Mooneye measures those same edges from the CPU side on
     both drawers - so it was not worth risking 86/86 for an interrupt no test asks about.
   - The **Standard** drawer still treats `WX` = 166 as one window pixel at x=159 on the line
     itself and never carries the window into the next line. That is wrong on hardware, but
     Standard is the default the owner plays on, every game here has been played on it, and no
     test covers it; changing the default drawer for a glitch edge case is not worth the risk.
     If Exact ever becomes the default this goes away with it.

## Needed in files this work does not own

Collected for whoever owns them. On the evening of 13 October `gb.rs` and `bus.rs` were this
work's to change, and the first item below was done then.

- **~~`gb.rs`: the Game Boy-compatibility colour palette~~ — DONE, evening of 13 October.** See
  Group 1, "Both halves of the palette are in". The three Mealybug pictures this said would pass
  the moment it landed did, and nothing else moved.
- **~~`bus.rs`: where an IO write lands inside its M-cycle~~ — DONE, 16 October.** See "How a
  register write reaches the drawer". The registers themselves still change at the end of the
  M-cycle, so Mooneye's gate and the Standard drawer are exactly where they were.
- **~~`gb.rs`: tell the PPU it is Color hardware~~ — DONE, by another agent, during this night.**
  `gb.rs` now sets `ppu.cgb_hardware` in all three places, Mooneye's Color set reads 7 of 7 runs,
  and `mts/misc/ppu/vblank_stat_intr-C.gb` is in the gate. **So the flag is live, and the two
  things in this file that were waiting on it are no longer blocked** — see Group 1 step 2 (six
  black-and-white Color pictures, each already measured to the pixel) and Group 1 step 2's drawer
  change for the palette. That is the next session's best-value work in this file. The original
  request is kept below only because it records what was measured and why.

  `Ppu` carries `pub cgb_hardware: bool` (`#[serde(skip)]`, so no save state changes size and
  `tests/fuzz.rs::state_layout_is_pinned` still holds), and the line 144 STAT rule above is
  written against it. `gb.rs` is the only file that knows which console this is. The lines, each
  directly under the one above it that already does the same thing for the bus:

  ```rust
  gb.bus.cgb_hardware = true;                                  // in with_model, the (Cgb|Agb, false) arm
  gb.bus.ppu.cgb_hardware = true;                              // <- add

  bus.cgb_hardware = true;                                     // in build(), colour mode
  bus.ppu.cgb_hardware = true;                                 // <- add

  next.bus.cgb_hardware = self.bus.cgb_hardware;               // in load_state
  next.bus.ppu.cgb_hardware = self.bus.ppu.cgb_hardware;       // <- add (it is the console, not the state)
  ```

  Since that was written the flag has become worth more than the one Mooneye ROM: it is also what
  the six black-and-white Color pictures in Group 1 step 2 are waiting on, and what the drawer half
  of the palette work above needs in order to be written at all.

  **This was measured, not assumed.** With those three lines applied in a sandbox,
  `mts/misc/ppu/vblank_stat_intr-C.gb` passes, Mooneye DMG stayed at its full count on *both*
  drawers (86 of 86 when that was measured; the gate is 87 now, for reasons nothing to do with
  this) and the gated Color set stays 6 of 6 runs. The lines were then taken back out, because this work owns
  neither `gb.rs` nor `bus.rs` and another agent was editing them the same night. Whoever applies
  them should also move that ROM out of the reported line at the bottom of `mooneye_cgb` in
  `tests/roms.rs` and into the gated list above it, which takes Mooneye Color to 5 of 5 ROMs — the
  comment there says so too. Nothing else in the core reads the flag, so there is nothing else to
  check.
- **`ppu.rs::mode3_length` is called from `tick` before the drawer runs**, so STAT and HBlank
  timing do not depend on which drawer is chosen. That is deliberate and must stay: Mooneye checks
  it from the CPU side on both drawers. The Exact drawer's own dot count now agrees with it.

## For the owner

cBoy's exact drawer — the one that draws a Game Boy picture a dot at a time, the way the real chip
does — was rebuilt from scratch this night instead of being patched again, and on the test set
that measures this sort of thing exactly it went from **5 pictures right out of 55 to 24**. That
24 is almost everything currently reachable: 23 of them are the whole Game Boy half bar one
picture that is four pixels away, and the 24th is the first of the Colour half, which is held up
by two things in files this work was not allowed to touch (both written down above for whoever
owns them, with the number of pictures each is worth). **It is still not worth switching on to
play games** — no real game has been played on it on a phone yet, so Standard stays the one to
use, and it stays the default either way.

Two things did change for both drawers, so they can show up in a game you play. They should only
ever look more right, but they are the only things here worth watching for:

- **Status bars and text boxes.** A real Game Boy only draws the window — the fixed panel games
  use for status bars and text boxes — if the window was switched on at the moment the screen
  passed the line it starts at. cBoy used to start it part-way down the screen when a game
  switched it on late; a real console waits for the next frame, which is now what cBoy does. If a
  status bar or text box ever seems to arrive a frame later than it used to, that is this change
  and it is worth telling Kiro.
- **Line timing, far smaller.** The console tells a game when it has finished drawing each line,
  and in two cases cBoy was saying so a few millionths of a second early: when a sprite sits hard
  against the left edge of a scrolled line, and on a Game Boy Color with sprites switched off.
  Both are now the real chip's timing. No game is likely to notice, and one that counts those
  moments closely can only be better off.

One more thing turned up that was worth doing for you rather than for the tests: cBoy's Colour
console played an old black-and-white game in plain grey, where a real Colour console tints it.
**That is done now (evening of 13 October).** A new choice under Picture, "Colour for
black-and-white games", plays old Game Boy games in the colours a Game Boy Color gives them, or
in one of the twelve a Color lets you pick with the buttons. It is off unless you pick it, and
the three test pictures it was meant to fix passed the moment it was in, which makes 27.

If you want to look at Exact anyway, on the phone: **Settings → Core settings → cBoy → Picture
drawing → Exact (experimental)**, then open a game. Switch it back to **Standard** to return to
the drawer every game here has been played on. Both drawers pass the same 87 timing tests and both
reference pictures, so nothing you can measure from inside a game should differ between them.
