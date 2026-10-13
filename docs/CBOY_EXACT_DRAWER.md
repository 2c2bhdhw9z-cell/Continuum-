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
| Mealybug Tearoom | not applicable | **5 / 55** |

The five Mealybug pictures that pass are all DMG: `m2_win_en_toggle`, `m3_bgp_change`,
`m3_lcdc_win_en_change_multiple_wx`, `m3_scx_low_3_bits`, `m3_wx_4_change_sprites`.

`cargo test --release --test roms mealybug_report -- --nocapture` now prints, for every failing
picture, how many pixels differ, how many lines they fall on, the first differing line, the first
differing pixel on it, and the colour drawn against the colour expected. A bare count cannot tell
a one-dot sampling error from a whole-screen one; this can.

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

Two consequences matter, and most of the failing pictures are one of them:

- **A register sampled on an address dot takes effect one dot later than a register used by the
  read.** This is what the `*2` variants probe. They place pairs of sprites at crafted X positions
  so the mid-line write falls on a known dot of the fetch (the source tables insert 12 to 19
  stall dots), then show which dot the change first reaches the screen on.
- **LCDC bit 4 and Y are sampled twice**, once per bitplane. A write in between mixes two
  different tiles, or two different rows of one tile, into a single row of pixels. The Mealybug
  documentation says Y is read at `B`, `0` and `1` on a DMG and on CGB revisions up to CPU CGB C,
  and only at `B` on CGB CPU D and the AGB — so `m3_scy_change` is a model difference between
  revisions, not a bug to fix twice.

### The window

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
background FIFO is non-empty. The object fetch then takes six dots, during which the background
fetcher is stalled. **Mode 3's length is the sum of those stalls**, not a formula — which is the
point of item C5, because `ppu.rs::mode3_length` currently computes it instead.

## The 50 failures, grouped

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
be guessing. The honest ceiling for this work is therefore the 24 DMG pictures, of which 5 pass.

### Group 1a — four CGB references that can never reach zero

`cgb:m3_wx_4_change`, `cgb:m3_wx_5_change`, `cgb:m3_wx_6_change` and
`cgb:m3_lcdc_win_en_change_multiple_wx` each contain **1146 pixels in 169 colours that a CGB
cannot output**: their channel values are not on the 5-bit ladder `(v << 3) | (v >> 2)` that every
real CGB colour sits on. The reference pictures were evidently captured through some blending.
All four currently report 1211 differing pixels, so they are already within 65 pixels of a floor
of 1146. **They must never be counted as achievable, and no change should ever be made to chase
them.** Note the DMG references for the same four tests are clean four-grey pictures, and
`dmg:m3_wx_4_change_sprites` already passes.

### Group 2 — the fetch address/read split (DMG: 5 pictures)

The single change with the widest reach. cBoy computes the address and performs the read on the
same dot (`fetcher_dot` does its VRAM access in steps 1, 3 and 5), so every register listed on an
address dot above is sampled one dot early.

| Picture | Differing | First difference | Register sampled on an address dot |
|---|---|---|---|
| `dmg:m3_lcdc_bg_map_change` | 956 px / 120 lines | line 0, x=8 | LCDC bit 3, at `B` address |
| `dmg:m3_lcdc_tile_sel_change` | 1032 px / 104 lines | line 40, x=8 | LCDC bit 4, at `0` and `1` address |
| `dmg:m3_lcdc_tile_sel_win_change` | 1240 px / 128 lines | line 0, x=0 | as above, window fetches |
| `dmg:m3_lcdc_win_map_change` | 630 px / 64 lines | line 0, x=0 | LCDC bit 6, at `B` address |
| `dmg:m3_scx_high_5_bits` | 84 px / 33 lines | line 8, x=16 | SCX bits 3-7, at `B` address |

### Group 3 — a value sampled twice in one fetch (DMG: 1 picture)

| Picture | Differing | First difference | Behaviour |
|---|---|---|---|
| `dmg:m3_scy_change` | 2192 px / 112 lines | line 5, x=8 | Y is re-read at `0` and `1`, so one row of pixels can mix two tile rows |

### Group 4 — the window (DMG: 5 pictures)

| Picture | Differing | First difference | Behaviour |
|---|---|---|---|
| `dmg:m3_lcdc_win_en_change_multiple` | 6066 px / 144 lines | line 0, x=114 | the window line counter must advance on every activation, not once per line |
| `dmg:m3_wx_6_change` | 13810 px / 140 lines | line 4, x=1 | WX moved mid-line; largest DMG failure |
| `dmg:m3_wx_5_change` | 627 px / 11 lines | line 13, x=7 | as above, plus the reactivation zero pixel |
| `dmg:m3_wx_4_change` | 221 px / 11 lines | line 12, x=12 | as above |
| `dmg:m3_window_timing_wx_0` | 522 px / 144 lines | line 0, x=10 | WX = 0 with an SCX fine scroll starts one dot later |
| `dmg:m3_window_timing` | 25 px / 10 lines | line 0, x=2 | the six-dot window startup fetch; closest DMG failure of all |

cBoy's present window code is a stack of special cases (`win_stop`, `win_lock`, `win_hold`,
`win_late`, `win_blank`, `wx_zero`, `wx_armed`) added one picture at a time. Sampling LCDC bit 5 at
the `B` address dot and advancing the window line on activation should replace all of them, which
is item C4's purpose.

### Group 5 — objects and the stalls they cause (DMG: 6 pictures)

| Picture | Differing | First difference | Behaviour |
|---|---|---|---|
| `dmg:m3_bgp_change_sprites` | 1168 px / 128 lines | line 0, x=8 | BGP written mid-line, object stalls shifting the dot it lands on |
| `dmg:m3_lcdc_bg_en_change` | 1231 px / 122 lines | line 0, x=2 | LCDC bit 0 mid-line, with object stalls |
| `dmg:m3_lcdc_obj_en_change` | 82 px / 66 lines | line 10, x=0 | LCDC bit 1 cleared mid-line |
| `dmg:m3_lcdc_obj_en_change_variant` | 330 px / 136 lines | line 0, x=150 | as above; BGP changed to expose the timing effect |
| `dmg:m3_lcdc_obj_size_change` | 175 px / 63 lines | line 2, x=27 | LCDC bit 2 mid-line: which dot reads object height |
| `dmg:m3_lcdc_obj_size_change_scx` | 50 px / 18 lines | line 2, x=27 | as above, with SCX changing the timing |
| `dmg:m3_obp0_change` | 58 px / 34 lines | line 50, x=6 | OBP0 written mid-line |

Six of the seven have objects placed at spread-out X positions purely to move the write's dot, so
they cannot pass before object fetch timing is right. `dmg:m3_lcdc_obj_size_change_scx` at 50
pixels and `dmg:m3_obp0_change` at 58 are the nearest.

### Group 6 — not probed by any failing picture

No DMG picture currently fails on VRAM/OAM access blocking (item C6): Mooneye already covers those
edges from the CPU side and passes 86/86 on both drawers. `m2_win_en_toggle`, the one `m2_*`
picture, already passes on DMG.

## Order of work, by pictures per change

1. **Group 2** — split every fetch into an address dot and a read dot. Five pictures depend on it
   directly and Groups 3, 4 and 5 all sit on top of it. This is item C2's real content.
2. **Group 4** — the window, once the fetch split exists. Six pictures, and `m3_window_timing` is
   25 pixels away.
3. **Group 5** — object fetch timing, and mode 3's length as a consequence of it. Seven pictures.
4. **Group 3** — the twice-sampled Y. One picture, and it is revision-dependent.
5. **Group 1** — only after `gb.rs` gains DMG-compatibility mode. 31 pictures, none reachable
   before that, and 4 of them not reachable at all.

## Needed in files this work does not own

Collected for whoever owns them; nothing here has been changed by this work.

- **`gb.rs`: CGB DMG-compatibility mode.** As set out in Group 1. Without it, 31 of the 55
  Mealybug pictures cannot move, and cBoy shows a white screen for any DMG game a player runs in
  Colour mode. This is worth doing for players, not only for the tests.
- **`ppu.rs::mode3_length` is called from `tick` before the drawer runs**, so STAT and HBlank
  timing do not depend on which drawer is chosen. That is deliberate and must stay: Mooneye checks
  it from the CPU side on both drawers. When the Exact drawer derives mode 3's length from its own
  stalls (item C5), the two must agree, and the formula stays for the Standard drawer.

## For the owner

Nothing has changed in how the app looks or plays. This was the groundwork for the exact picture
drawer: the test harness now says exactly *where* each of the 50 unfinished test pictures is
wrong, instead of only that it is wrong, and the real cause of the biggest group has been found
and written down.

**Exact is still not worth switching on**, and it stays off by default. Of the 55 test pictures it
still passes 5. Worse, 31 of them turn out to be blocked by something separate: cBoy does not yet
colour an old black-and-white game the way a Game Boy Color does, so those games come out white in
Colour mode. That is a real bug worth fixing for players, and it is now written down for the
session that owns that file.

If you want to look at Exact anyway, on the phone: **Settings → Core settings → cBoy → Picture
drawing → Exact (experimental)**, then open a game. Switch it back to **Standard** to return to
the drawer every game here has been tested on.
