# cArcade: Continuum's own arcade emulator

**Where it stands (14 October 2026): the Z80 is proven (item I2: all 1,604,000 SingleStepTests
cases pass, and ZEXDOC and ZEXALL report no errors); Donkey Kong's 8035 and Konami's AY-3-8910
match MAME (their sections below); the Pac-Man board is proven (item I3: nine test programs
of our own match MAME on every one of 1,400 frames, pixel for pixel and sound sample for sample;
see "The Pac-Man board, as proven"); and cArcade is in the app (item I4): arcade games open on it,
and it plays 65 sets on the Pac-Man board (see "In the app").**
The write-up at the end of the job finishes this file: the games it plays, what the tests prove,
what is approximate, and what comes next.

Why it exists: on 12 October the owner took the two arcade emulators with non-commercial licences
out of the app (FinalBurn Neo and MAME 2003-Plus) and ruled out any outside arcade core for good.
Arcade comes back only on an emulator of Continuum's own, the way cBoy is its own Game Boy
emulator. Until then an imported arcade game stays in the Library and opens to one line:
"Arcade games aren't playable right now. We're building our own arcade emulator."

## Name and shape

- **Name in the app: cArcade**, a working name to match cBoy. The owner can rename it; the name
  lives in two places, the core's `library_name` and the `displayName` of its `CoreSpec`.
- Core id `continuum_arcade`, file `continuum_arcade_libretro_ios.dylib`.
- Crate `cores/continuum-arcade`, crate name `continuum_arcade`: a standalone libretro core like
  `cores/continuum-gb`, with its own empty `[workspace]`, GPL-3.0-only, `cdylib` + `rlib`, and
  cBoy's dependencies and settings (serde, bincode, libc; tests at opt-level 2). Pure Rust, no C.
- Our own code. Each board is written from hardware documentation and from how MAME models it,
  never copied from MAME. Every piece gets a short comment naming the hardware behaviour it
  reproduces.

## One module per piece

| Module | What it is |
|---|---|
| `z80.rs` | The Z80, shared by all three boards and by Konami's sound board |
| `zip.rs` | Reads a zip: its directory (names, sizes, CRC32s, ZIP64 too), and its files, stored or deflated, with our own inflater and CRC32, so no new dependency (written) |
| `sets.rs` | The recognition rules below, and what a set's chips are and where they load (written) |
| `set_list.rs` | The built-in set list: each set's name, title, parent, board, switches, and each chip's name, size, CRC32 and place. Written by `scripts/make-set-list.py` from MAME's driver source and `-listxml` (facts about the chips, no MAME code) |
| `board.rs` | What every board does for the core: run one frame with the controls and switches, the picture, the sound, save states; the four-way stick (written) |
| `pacman.rs` | Namco's Pac-Man board (written and proven, see below) |
| `namco_wsg.rs` | Pac-Man's sound chip: three voices playing 32-step waves from the sound PROM (written and proven with the board) |
| `galaxian.rs` | Namco's Galaxian board and Konami's Scramble variant |
| `ay8910.rs` | The AY-3-8910, two of them on Konami's sound board (written and proven, see below) |
| `galaxian_sound.rs` | Galaxian's and Moon Cresta's analogue sound, approximated (written, see below) |
| `dkong.rs` | Nintendo's Donkey Kong board |
| `mcs48.rs` | The Intel 8035 that plays Donkey Kong's music into a DAC (written and proven, see below) |
| `dkong_sound.rs` | Donkey Kong's analogue sound effects, approximated |
| `libretro.rs`, `options.rs` | The entry points, and the DIP switches as core options (written) |

## How a game is recognised

Arcade games are zips named after the game (`pacman.zip`), holding the board's ROM chips.

1. **Read the zip's own directory.** Each file's name, size and CRC32 is in it, so recognising a
   set needs no unpacking. The folder part of a name doesn't matter, so chips inside a merged
   set's clone folders are found too.
2. **Find each ROM by CRC32 and size, never by file name.** That makes merged, split and
   non-merged sets the same problem. A non-merged zip holds everything itself. A split clone's zip
   holds only the chips that differ from its parent, so the rest come from the parent's zip. A
   merged parent's zip holds its clones' chips too.
3. **A clone's parent is found next to it**: `<parent>.zip` in the same folder, any letter case.
   That's why the core needs the zip's path as well as its bytes. With `need_fullpath` false the
   engine already passes both: it reads the file into memory and keeps the full path in
   `info->path` (`cores/native_core.rs`, `load_content`).
4. **Which set it is:** the set the zip is named after, if all its ROMs are there. If the name is
   unknown (a renamed zip), it's the known set whose ROMs are all there, and the one with the most
   chips in the zip itself wins, so a clone beats its parent. A merged parent's zip plays the
   parent, the same as in MAME.
5. **When a chip's CRC differs:** if the zip is named after a known set and every file that set
   needs is there by name and size, it still plays. That's a homebrew or a hack on that board,
   which MAME also runs (with a warning). The log names the chips that differ.
6. **Otherwise it refuses with one plain sentence**, never a crash. For a game it doesn't know:
   "Our arcade emulator can't play this one yet. Right now it plays Pac-Man, Ms. Pac-Man, Galaxian,
   Scramble, Frogger, Amidar and games on the same boards." The sentence is built from the boards that are switched on. For a split
   clone with no parent: "This one needs mspacman.zip next to it." For a missing or bad chip:
   "This copy of Pac-Man is missing a piece. A complete copy of pacman.zip fixes it."

A board is **switched on only once it is proven**. Until then its sets are left out of the list,
so its games get the "can't play this one yet" sentence instead of a broken game.

## The boards, in order

1. **The Z80** (item I2).
2. **Pac-Man** (I3): Puck Man, Pac-Man, Ms. Pac-Man with its daughterboard (which rearranges and
   patches the program ROMs), Pac-Man Plus (its program and graphics decoding), and the common
   clones and bootlegs on the same board. A Z80 at 3.072 MHz; 224×288 upright; tiles and 8 sprites
   with the board's unusual screen layout; colour and palette PROMs; the Namco sound chip; the
   coin, start and four-way stick; the DIP switches; the watchdog; and the interrupt vector the
   game writes to port 0.
3. **In the app** (I4): the libretro core around the board, and the wiring below.
4. **Galaxian and its family** (I5): Galaxian, Moon Cresta and their clones, and Konami's variant
   for Scramble, Super Cobra, Frogger and Amidar. That variant adds a second Z80 with two
   AY-3-8910s. Covers the tilemap, sprites, bullets, starfield and colour PROM. Galaxian's own
   analogue sound is approximated from the circuit, and this file will say so.
5. **Donkey Kong** (I6): Donkey Kong and Donkey Kong Jr. A Z80 for the game and an 8035 playing
   the music into a DAC. The analogue effects are approximated, and this file will say so.
6. **The write-up** (I7).

All three boards run at **60.606 Hz**: a 6.144 MHz pixel clock over 384 × 264 clocks per frame.
All three are vertical games, so the picture is **3:4**. The core draws it upright itself instead
of asking the frontend to rotate it, so screenshots, the TV picture and rewind all come out
upright too.

## How each piece is proved

- **The Z80** has three proofs:
  - **SingleStepTests' Z80 suite** (MIT-licensed JSON, one file per opcode, 1000 cases each).
    Every case must pass on every register, both undocumented flag bits, WZ (MEMPTR), R, Q, the
    interrupt flip-flops, memory, the order and values of every memory and I/O access, and the
    cycle count (`tests/z80_singlestep.rs`, about 2 seconds).
  - **ZEXDOC and ZEXALL**, run through a tiny CP/M stand-in in the test harness (BDOS calls 2 and
    9 at address 5). Both must report all 67 groups OK (`tests/z80_zex.rs`, about 45 seconds:
    5.8 billion instructions each).
  - **The interrupts, HALT and the refresh register**, which neither suite reaches: unit tests in
    `src/z80.rs` (modes 0, 1 and 2, the NMI and RETN, EI's one-instruction delay, HALT, the
    LD A,I quirk, a run of prefixes, R's bit 7, and a save state).

  `cores/continuum-arcade/scripts/fetch-test-suites.sh` downloads them (about 290 MB, nearly all
  of it the JSON) into the crate's git-ignored `test-suites/` and checks each against a sha256;
  they're never committed. Without the download those tests say SKIPPED and pass, the way cBoy's
  test ROMs work.
- **Each board** is proved with programs we write ourselves:
  - **The test programs** draw tiles and sprites with our own graphics and colour PROMs, play our
    own waves, read the inputs and DIP switches, and exercise the watchdog and interrupts. They're
    in Z80 assembly (sjasmplus, built from source), plus 8035 assembly for Donkey Kong's sound
    (The Macro Assembler AS does the 8035).
  - **MAME is the reference.** The same programs run on MAME 0.289 built locally from its release
    tarball as a subset holding only the three boards' drivers (MAME's own `SOURCES=` build,
    `-j2`), in `/projects/sandbox/.scratch/mame` (its `READY` file says how to run it). It is
    never committed or shipped.
  - **The tests.** MAME's per-frame picture and sound hashes for each program are committed as
    small reference files in the crate's `tests/references/`, which every run checks; whenever
    MAME is on the machine, a second test compares against MAME live, frame by frame. The
    Pac-Man board's tests are described above; they need no tolerance at all.
  - **Licensing.** The programs' sources and assembled bytes are ours and are committed. No
    commercial ROM is ever fetched, not even for a test.
- **Save states** must round-trip byte for byte, and the frames after a restore must equal the
  frames without one.
- **Through the engine**, `crates/emulator-bridge/tests/arcade_frontend.rs` loads a test set built
  from our own programs through the real entry points (`NativeLibretroCore`). Like
  `cboy_frontend.rs`, it only runs when it's given the built core.

## The 8035, Donkey Kong's sound processor (`mcs48.rs`)

The Intel MCS-48 family as Donkey Kong and Donkey Kong Jr. use it: the 8035 (64 bytes of RAM)
and the 8039 (128). A step is one instruction; a machine cycle is 15 clocks, so the boards' 6 MHz
part runs 400,000 cycles a second.

- **What's in:** every instruction and its cycles, both register banks, the eight-level stack in
  RAM, the PSW and its flags, F1, the timer and event counter and their interrupt, the external
  interrupt, ports 1 and 2, the BUS, T0 and T1, MOVX, the 8243 expander instructions, and the two
  2 KB program banks (SEL MB0/MB1; Donkey Kong Jr.'s 4 KB sound program uses both).
- **Where Intel's manual leaves it open, MAME's core decides:** the timer counts an instruction's
  cycles before the instruction acts, an interrupt that arrives just after a JNI that missed it
  returns to the JNI's destination, and the 26 undefined opcodes are one-cycle no-ops. MAME's own
  notes say a real 8048's timer runs one cycle later than that; matching MAME keeps the
  comparisons with it exact.
- **How it's proved.** There is no public per-instruction suite for this chip (SingleStepTests
  has none), so:
  - `tests/mcs48_opcodes.rs` checks all 256 opcodes against Intel's 1980 MCS-48 User's Manual.
    Each instruction's operation is written out a second time from the manual, keyed by its
    mnemonic, and the chip and that description run from 1,500 random machine states per opcode
    on each RAM size (768,000 cases). Every register, flag, RAM byte, port latch, timer bit, outside
    access and cycle count must agree. Planted mistakes (a wrong half carry, a missing bank rule, a
    port read without its latch, a count carrying into bit 11) all fail it.
  - The opcode table is checked against an independent assembler: `tests/mcs48/opcodes.asm` has
    every instruction, and `opcodes.bin` is what The Macro Assembler AS made of it.
  - Unit tests in `src/mcs48.rs` cover the interrupts and their priority, RETR, bank 0 inside an
    interrupt routine, the JNI rule, the timer's 32-cycle prescaler, the counter's T1 edges, the
    stack's wrap, the page rules, both RAM sizes, reset and a save state.
  - `tests/mcs48_mame.rs` compares it with MAME's MCS-48 core, instruction by instruction. Four
    programs of ours (`tests/mcs48/alu.asm`, `flow.asm`, `timer.asm`, `io.asm`: every
    accumulator value through the arithmetic, all the jumps, banks, stack and PSW values, the
    timer, counter and both interrupts, and the ports, MOVX and BUS as Donkey Kong's board wires
    them) ran on MAME 0.289's Donkey Kong sound board with its debugger tracing the program
    counter, A, PSW, R0-R7, timer, prescaler, both ports and the cycle count before each
    instruction. All 104,360 instructions match. The `.mame` files keep a hash of each 1,024
    lines; `tests/mcs48/mame_trace.py` remakes them from a MAME build in scratch.

## The AY-3-8910, Konami's sound chip (`ay8910.rs`)

General Instrument's sound chip, two of which play Scramble, Super Cobra, Frogger and Amidar.
It divides its clock by 8; `tick` is one period of that and gives the three channels' levels.

- **What's in:** three tone generators (a square wave of 16 × TP clocks), the 17-bit noise
  generator (taps at bits 0 and 3, a shift every 16 × NP clocks), the envelope generator with all
  16 shapes (each step 16 × EP clocks), the mixer, fixed or envelope amplitude per channel, the
  address latch with the chip's own code in its top four bits, what each register reads back, and
  the two I/O ports through which Konami's boards read the sound command and a timer.
- **The output levels** are the data manual's (Fig. 9): 1, .707, .5, .303, .25, .1515 and .125 V
  for levels 15 down to 9, halving every two steps below that, level 0 off. A board can use them
  or model its own output stage; the chip itself hands out the levels.
- **Where the manual leaves it open, MAME's core decides:** the counters count up and wrap when
  they reach their period (so lowering a period takes effect at once), a tone or noise period of
  0 is 1, an envelope period of 0 steps twice as fast as 1, and writing the shape restarts the
  envelope.
- **How it's proved:** unit tests in `src/ay8910.rs` against the data manual: tone periods from
  1 to 4095, period 0, the noise sequence bit for bit against an independent shift register and
  its 131,071-shift length, all 16 envelope shapes over four cycles, the envelope step length,
  the mixer, each channel on its own, the register masks, the chip code, the ports, reset and a
  save state.
- **And against MAME, sample for sample** (`tests/ay8910_mame.rs`): `tests/ay8910/jumpbug.asm`,
  a Z80 program of ours, writes a script of registers on Jump Bug's board, where MAME clocks the
  chip at 1.536 MHz so its output is exactly 192,000 samples a second. MAME 0.289 recorded it at
  that rate: every channel's tone at many periods, the noise at many periods and through each
  channel, all 16 envelope shapes, envelope periods 0 to 300, periods and shapes changed
  mid-wave, the missing register bits and the mixer's combinations. Run on our Z80 and chip, with
  each write landing on the sample MAME's does, all 48,715 samples match. MAME turns levels into
  voltages with its own model of the output stage, so the reference keeps its value for each
  level, taken from a calibration at the start of the script. Planted mistakes (other noise
  taps, the envelope period-0 rule, the tone period-0 rule, a shape write resetting the envelope's
  prescaler, a write one sample late) all fail it. `tests/ay8910/mame_ay.py` remakes the
  reference from a MAME build in scratch.

## The Pac-Man board, as proven (item I3)

`src/pacman.rs` and `src/namco_wsg.rs`. The board takes its ROM regions as MAME's sets lay them out
(the set list in item I4 fills them) and one of four program layouts: the plain board (Puck Man,
Pac-Man and the clones on it; 16 KB at 0000h, mirrored at 8000h), Pac-Man Plus (the same, every
program byte decoded by its address), bootlegs that wire A15 (a second 16 KB at 8000h, as many
Ms. Pac-Man bootlegs do), and Ms. Pac-Man's daughterboard (its three scrambled ROMs, the 40
patches it lays over the Pac-Man program, and the traps that switch between the two programs on
any read, write or instruction fetch).

What it does: the Z80 at 3.072 MHz, 50,688 cycles a frame, 60.606 frames a second; the 36 × 28
tile layer with its odd video RAM layout, its mirrors and the screen flip; the eight sprites with
the board's clipping, wrap-around, priorities, see-through colour and the one-pixel shift of
sprites 0-2; the palette from the PROM through the resistor weights; the picture turned upright,
224 × 288; the three-voice sound from the sound PROM; the inputs, DIP switches and cabinet
setting; the VBLANK interrupt held until the game clears it, with the vector from port 0 in
modes 0, 1 and 2; and the watchdog's reset after 16 frames without a kick (RAM kept, the latch
cleared). Save states round-trip byte for byte, and the frames after a restore equal the frames
without one. Pac-Man Plus's graphics aren't scrambled (MAME's set loads them plain), so only its
program is decoded.

How it's proved:

- **Nine test programs of our own** in `test-programs/pacman/` (Z80 assembly, with our own
  graphics and PROMs: `video`, `sprites`, `sound`, `irq`, `watchdog`, `inputs`, `mspacman`,
  `pacplus`, `upper`), assembled with sjasmplus 1.24.0 built from source by
  `scripts/build-test-programs.sh`. The assembled bytes are committed; a test checks they are
  what the sources assemble to whenever the assembler is there. The test scrambles the
  Ms. Pac-Man and Pac-Man Plus programs the way those boards' ROMs are scrambled, with its own
  copy of the wiring, so cArcade's decoder must undo MAME's idea of the scrambling, not its own.
- **MAME 0.289 as the reference**, built locally from its release tarball with only the Pac-Man,
  Galaxian and Donkey Kong drivers (`SUBTARGET` + `SOURCES`, headless SDL), never committed or
  shipped. `tests/pacman_mame.rs` runs each program on MAME (its files under MAME's names in
  MAME's `pacman`, `mspacman`, `pacplus` and `mspacmab` sets; MAME runs them after its
  wrong-checksum warning), snapshots every frame, records the sound at 192 kHz, presses the same
  buttons and sets the same DIP switches frame by frame through MAME's Lua console, and compares
  every frame with cArcade's: the picture pixel for pixel, the sound sample for sample (the
  voices' sums, recovered exactly from MAME's 16-bit samples). Without MAME it says SKIPPED.
- **Pinned for everywhere**: the same test writes MAME's per-frame picture and sound hashes into
  `tests/references/pacman/` (`CARCADE_WRITE_REFERENCES=1`) and otherwise checks those files are
  still what MAME produces; `tests/pacman_board.rs` runs on any machine and requires cArcade's
  every frame to hash the same. All 1,400 frames match.

Where it follows MAME's model rather than a measured board (the reference has to be matched
exactly to be useful, and none of these can be heard or seen):

- The sound runs at 192 kHz, twice the chip's step rate with one more fraction bit (the same
  pitch), and a voice's phase only moves while the sound is on and its volume isn't 0.
- A sound register write changes the 192 kHz samples that start at or after it; VBLANK falls in a
  frame's last CPU cycle.
- CPU writes to the phase-counter nibbles the chip keeps in its register file are stored but don't
  move the voices; the 3M timing PROM isn't modelled; the lamps, coin counter and coin lockout
  have no effect.

Known limits, none of which the tests hit:

- An instruction that straddles VBLANK has its writes placed in time from the instruction's
  shape (exact for ordinary stores and the LDI family, close for EX (SP),HL); a watchdog reset
  that lands mid-instruction waits for the instruction's end (exact from HALT).
- MAME switches the sound on or off from its sound stream's last update rather than at the write
  (its `sound_enable_w` doesn't bring the stream up to date first); cArcade switches it at the
  write, as the latch does. The test programs only switch it while every voice is silent, where
  both agree.
- Two bootlegs with their blue and green wires swapped (MAME's `woodpek_rbg` sets) aren't
  modelled; they'd need a palette variant.

## The Galaxian board, as proven (item I5, picture and timing)

`src/galaxian.rs`: Namco's Galaxian board and Nichibutsu's Moon Cresta (the same board with its
memory moved, a graphics bank and, on Nichibutsu's own boards, a scrambled program). Its sound is
in the next section. Not in the app yet: Konami's variant comes first.

What it does: the Z80 at 3.072 MHz; the 32 × 32 tile layer with each column's own scroll and
colour; the eight sprites, with the line buffer's clipping and sprites 0-2 a line lower; the
seven shells and the missile; the star field (the 17-bit generator, its 64 colours and its
sideways drift); the colour PROM through the resistor network; both screen flips; drawing as
the beam goes, so a change mid-frame splits the picture where the real board would; the NMI ON
latch and the Z80's edge-triggered NMI; the watchdog's reset after 8 VBLANKs without a read;
the inputs, DIP switches and the coin lockout (a locked-out coin is turned away, so it never
reaches the coin switch). The picture is kept at three dots a pixel along the raster, as MAME
draws it, because the stars change colour a third of the way into a pixel: 224 × 768 upright.

How it's proved, the same way as Pac-Man:

- **Ten test programs of our own** in `test-programs/galaxian/` (`tiles`, `sprites`, `shots`,
  `stars`, `raster`, `nmi`, `watchdog`, `inputs`, and `mooncrst` run twice: on Nichibutsu's
  scrambled set, with the scrambling done by the test from its own copy of the wiring, and on a
  plain one), with our own graphics and colour PROM.
- `tests/galaxian_mame.rs` runs each on MAME 0.289 and on cArcade and compares every frame pixel
  for pixel: all 1,800 frames match. `tests/galaxian_board.rs` runs everywhere against MAME's
  recorded hashes in `tests/references/galaxian/`, round-trips save states mid-run, and checks the
  committed programs are what their sources assemble to.
- The NMI program was also compared instruction by instruction with MAME's debugger trace: all
  182,314 instructions agree, with the cycle each one started on.

Where it follows MAME's timing rather than a measured board (found on 16 October; each is a
real effect a test program could see, and the Pac-Man board's tests still pass with them):

- The Z80 comes up with IX and IY at FFFFh (MAME's power-on value; a game that pushes them
  before loading them shows it).
- At VBLANK the NMI line rises before the watchdog counts. A watchdog reset then drops the NMI
  the Z80 had latched but leaves the line high, so no NMI comes until the game drops the NMI ON
  latch and VBLANK raises the line again.
- The reset cuts into the instruction running at VBLANK where MAME's Z80 had stopped for VBLANK
  (it can pause halfway through an opcode fetch, after any bus cycle, or after an internal
  cycle), and the Z80's clock carries on from there.
- Switching the NMI off cuts MAME's CPU timeslice right after the write, which moves where the
  next VBLANK falls by a cycle in some frames (`vblank_cycle`).

## Konami's Scramble boards, as proven (item I5)

`src/galaxian.rs` (`Variant::Konami`), `src/i8255.rs` and `src/konami_sound.rs`: the Galaxian board
as Konami built it for Scramble, The End, Super Cobra, Frogger, Amidar and Turtles, each with its
own memory map (MAME's scramble, theend, scobra, frogger and turtles machines).

- **Main board**: two 8255 PPIs (`i8255.rs`, mode 0: the first reads the controls and DIP switches,
  the second sends the sound board its command on port A and its control on port B, and reads IN3
  or the protection chip on port C); 2 KB of RAM; 16 or 32 KB of program. The pictures: Scramble's
  and Super Cobra's two-dot yellow shells, blue background switch and still stars that blink
  through four states on a 555 (0.8316 s); The End's Galaxian-style shots with blue and green
  swapped; Frogger's river (blue on one half), its colour bits wired one place round, its scroll and
  sprite rows with their nibbles swapped and its second graphics ROM's D0/D1 swapped; Turtles' and
  Amidar's background in any of eight colours. The protection PAL on Scramble's and The End's port C
  is modelled as MAME has it (its logic isn't documented; this is what the games expect).
- **Sound board** (`konami_sound.rs`): its own Z80 at 1.79 MHz, 1 KB of RAM, two AY-3-8910s (one on
  Frogger, whose first sound ROM has D0/D1 swapped), the command latch and the interrupt (bit 3 of
  the main board's port B falling; held until taken), the timer chain through the first AY's port B
  (Frogger's with two bits swapped), bit 4 silencing the sound, and the filter latch (twelve switches
  from the address of a write to 9000h-9FFFh). A watchdog reset resets the whole board, the sound
  Z80 and the PPIs too, as MAME's soft reset does.
- **The filters and amplifier are an approximation**: each channel's AY output (its output
  transistor as a resistor per volume level, from the transconductances MAME's AY model fits to a
  real chip), 1 kΩ, a node with 0.22 µF and 0.047 µF each behind a 270 Ω switch, 5 kΩ into an
  op-amp summing all six (2.2 kΩ feedback, ±3.5 V swing), divided down and coupled into the
  amplifier. It's stepped on every AY tick and averaged to 48 kHz. MAME runs the same circuit
  through its netlist simulator; the two agree within 7% in level.
- **Timing, as MAME has it**: the sound Z80 sees a command or an interrupt from the first
  instruction it starts at or after the moment the main CPU's write ended (MAME's scheduler stops
  the main CPU there and runs the sound CPU up to it before the latch changes). A watchdog reset
  that lands mid-instruction cuts the sound Z80's instruction where MAME's had paused. Bus accesses
  happen at the start of their bus cycle, as in MAME's Z80, which is when the timer is read and
  when an AY write lands. The Z80 now powers on with MAME's register values (AF 0040h, SP 0000h).

How it's proved, the same way as the Galaxian board:

- `test-programs/konami/`: `kmain.inc` on each map (`kscramble` for Scramble's and The End's,
  `kscobra`, `kfrogger`, `kturtles`) moves tiles, sprites and shots, flips the screen, switches the
  stars and backgrounds, changes things mid-frame, shows every input and the protection's answers
  as tiles, sends a command every 8 frames, silences the sound for 20 frames and lets the watchdog
  reset the board at frame 150; `ksound.inc` (`ksound` on the two-AY board, `kfsound` on Frogger's)
  plays tones on every channel of both AYs with each filter switch, two at once, all six and the
  noise, timing itself on the timer, and reads each command in its interrupt.
- `tests/konami_mame.rs` runs them on MAME 0.289's `scramble`, `theend`, `scobra`, `frogger` and
  `amidar` sets: **all 260 frames of each match pixel for pixel, and all of the sound Z80's
  instructions (904,936, or 904,864 on Frogger) match MAME's debugger trace line for line**:
  cycle count, AF, BC, DE, HL, SP, IX and PC, through the watchdog's reset. Since the AY itself
  matches MAME sample for sample (above), the AYs here get the same writes at the same cycles; their
  output can't be compared sample for sample on this board because MAME feeds it into its circuit
  model. The sound after the filters is compared segment by segment: every tone's strongest
  frequency within 0.2% (they agree exactly), its level within 8% (1-7% apart) and its spectrum at
  least 0.99 alike. MAME 0.289's recording doesn't apply the board's silencing bit (its system mute
  only reaches the speakers), so those frames aren't compared; cArcade silences the sound there, as
  the board does.
- `tests/konami_board.rs` holds cArcade to MAME's recorded hashes and measures in
  `tests/references/konami/` everywhere, round-trips save states after the reset, and checks the
  committed programs assemble from their sources. Planted mistakes (the 0.047 µF filter as 0.1 µF,
  the timer's B4 from the wrong counter bit, Frogger's colour wiring left out) all fail it.

## Galaxian's sound, approximated (item I5)

`src/galaxian_sound.rs`. **This is an approximation.** Galaxian and Moon Cresta have no sound
chip: their sound is analogue circuitry on the main board (555 timers, a resistor DAC, a noise
shift register, op-amp filters), switched by latches the game writes. cArcade simulates how that
circuit behaves, from the component values on the schematics (as MAME's model of the board lists
them in `galaxian_a.cpp`), in its own code. It isn't a recording of a real board, and only the
tone's counters are exact.

- **The tone** (exact): the pitch latch loads counters on the 1.536 MHz sound clock that step a
  74393 every 256 - pitch clocks (FFh stops them); QA, QC and QD are mixed, QC again through
  VOL1's 10 kΩ and QD only through VOL2's 15 kΩ.
- **The background**: the LFO latches drive a four-bit resistor DAC, which sets the current that
  ramps a 555's 1 µF capacitor (a sawtooth every 1.8 to 13 seconds); an op-amp makes that the
  control voltage of three 555s (FS1-FS3, held in reset while their latch is 0).
- **The noise**: a 17-bit shift register (stage 4 XOR the inverse of stage 16) on 12.288 MHz,
  sampled every two raster lines. MAME runs its generator 100 times slower to save time; cArcade
  steps it at the board's clock (1,536 steps between samples), which costs nothing with a table.
- **HIT** (the explosion): the noise switches a capacitor that HIT charges and that decays through
  172 kΩ, into a band-pass filter around 168 Hz. **FIRE** (the shot): a 47 µF capacitor and the
  noise sweep a fourth 555, which switches a capacitor FIRE charges.
- **The mix**: Galaxian's network (5.1 kΩ twice, 10 kΩ load) or Moon Cresta's (one network, 15 kΩ
  for the background, no load), then the amplifier's coupling capacitor. It runs at 192 kHz and
  the core hands out the mean of each four samples (48 kHz).

How it's proved, as far as an approximation can be:

- `test-programs/galaxian/sound.inc` (in `sound.asm` on Galaxian's map and `mcsound.asm` on Moon
  Cresta's) plays the latches in 28 segments: the tone at four pitches with each VOL1/VOL2
  setting, each background oscillator alone and with LFO bits set, HIT and FIRE and their tails,
  with quiet between. `tests/galaxian_mame.rs` records MAME 0.289's sound at 192 kHz and compares
  each segment (`tests/galaxian/sound.rs`):
  - the tone's strongest frequency within 0.2% of MAME's and of the counters' own arithmetic, and
    its level within 1% (they agree to 0.2%), which checks VOL1, VOL2 and the mixing resistors;
  - the background's level within 3% and its spectrum (twelfth-octave bands) at least 0.95 like
    MAME's (they come out 0.998 or more: the sweep runs in step with MAME's for the whole run);
  - HIT and FIRE and their tails: level within 10% (they're 1-9% apart) and octave-band spectrum,
    averaged over short windows, at least 0.98 like MAME's (0.996 or more). Noise can't be compared
    sample for sample: the two generators are different models of it.
  - Quiet is under a millivolt in both.
- MAME's measures are kept in `tests/references/galaxian/sound.txt` and `mcsound.txt`, and
  `tests/galaxian_board.rs` holds cArcade to them everywhere, without MAME. Planted mistakes (VOL1's
  resistor 11 kΩ, the counters one clock long, FIRE's 2.2 kΩ as 4.7 kΩ) all fail it.
- Unit tests: the noise register against a second, stage-by-stage version of the schematic's,
  its 131,071-clock period and balance, its sampling every 1,536 clocks; the counters' rate at six
  pitches and FFh holding them; the background ramp's period against its current; that every
  latch bit (FS1-3, HIT, FIRE, VOL1, VOL2, the pitch and each LFO bit) changes what comes out; that
  HIT and FIRE die away; and a state round trip.

## In the app (item I4)

### What it plays

65 sets on the Pac-Man board and 119 on the Galaxian family's boards, every one MAME runs on
exactly the hardware cArcade models: Puck Man, Pac-Man, Pac-Man Plus, Ms. Pac-Man and 61 of their
clones, hacks and bootlegs; and Galaxian, Moon Cresta, Scramble, The End, Super Cobra, Frogger,
Amidar, Turtles and their clones (see "The Galaxian family in the app" below).

- On the plain board: `puckman` and its clones (`pacman`, `pacmanf`, `pacmod`, `puckmod`,
  `hangly`, `newpuckx`, `pacheart`, `crockman`, `joyman`, `piranhah` and 24 more).
- `pacplus` with its decoder.
- On Ms. Pac-Man's daughterboard: `mspacman`, `mspacmnf`, `mspacmat`, `msheartb`, `pacgal2`,
  `mspacmancr`.
- Bootlegs with ROM at 8000h: `mspacmab` and 22 more, among them `pacgal`, `mspacpls` and the
  "Made in Greece" boards, whose single big EPROMs `set_list.rs` splits the way MAME loads them.

Left out for now, because their hardware isn't modelled: sets whose program is encrypted (Eyes,
Piranha, Mr. TNT and the encrypted Ms. Pac-Man bootlegs), the two with blue and green swapped,
and the driver's other machines (Crush Roller, Ms. Pac-Man Champion Edition, Ponpoko, ...).

### The core

- `valid_extensions` is `zip` (7z is not in the first version). `need_fullpath` is false, so the
  core gets the bytes and the path (see step 3 above); `block_extract` is true.
- A zip it can't play is refused with one sentence, logged as `cArcade: <sentence>` just before
  `retro_load_game` returns false: the four in "How a game is recognised", and for a file that
  isn't a working zip, "This file isn't a working arcade zip. Another copy of the game usually
  fixes it." The log line before it says what the zip held. A known set's zip whose chips differ
  (a hack, a homebrew, our own test programs) plays, with the differing chips logged.
- The picture is XRGB8888, upright, 224×288, at a 3:4 aspect, 60.606 fps; the sound is 48 kHz,
  the board's mono on both channels.
- **Save states:** `CARC`, the format (1), the set's name, then the board's bincode state. The
  format is in `library_version` ("0.1.0 (state format 1)"), as cBoy does, so the app refuses a
  state from another layout; a state of another set is refused too. There are no battery saves:
  these boards have no battery memory.
- **Restart** switches the cabinet off and on again: the game starts as fresh.
- **DIP switches** are core options in a "Pac-Man games" group, with the factory settings as
  defaults (a test holds all 65 sets' defaults to MAME's, bit for bit): `carcade_pacman_coinage`
  (1 coin 1 game, 2 coins 1 game, 1 coin 2 games, free play), `_lives` (1, 2, 3, 5), `_bonus`
  (an extra life at 10,000, 15,000 or 20,000 points, or none), `_difficulty`, `_ghosts` (Pac-Man's
  second set of ghost names) and `_cabinet` (upright or cocktail table). Once a game loads, the
  ones its cabinet doesn't have are hidden (Ms. Pac-Man has no ghost names switch). Where a
  bootleg's switch means something else, it isn't offered and stays at the factory setting: Come
  Come's coinage and lives, Newpuc2's bonus scores, Pacu-Man's coinage, and the Spanish Puck Man's
  difficulty (its Hard setting breaks the game). The rack test, service coin and service mode
  switches stay off.
- **Controls** (`read_controls`): the D-pad is the stick, Select is the coin, Start is that
  player's start, B A Y X L R are the panel's buttons one to six, and X on the first pad is also
  player 2's start. The second pad is player 2's side of a cocktail table, with its own coin.
  On a four-way stick a diagonal becomes the newer of its two directions, as the stick's gate
  would (straight into a diagonal, it goes sideways).
- Log lines start with `cArcade: `.

### How I4 is proved

- `tests/libretro.rs` drives the core only through its C entry points, with a stand-in frontend:
  - each of the nine Pac-Man board test programs, zipped under the file names of MAME's set
    (`pacman.zip`, `mspacman.zip`, `pacplus.zip`, `mspacmab.zip`), plays every frame to the same
    picture MAME 0.289 drew (its recorded hashes), and the sound is the board's on both channels;
  - the inputs program is played through the frontend's two pads and the DIP switch settings, and
    every frame matches MAME's except the 10 where the test has a switch cArcade doesn't offer
    turned on (a planted mistake, left and right swapped, fails it);
  - a split `pacman.zip` asks for `puckman.zip`, and with `PuckMan.zip` beside it plays MAME's
    frames; an unknown zip, a zip missing a chip and a file that isn't a zip each get their
    sentence; save states round-trip byte for byte with the same frames after; Restart gives a
    fresh start's frames; the settings, the hidden switches and the av info are checked; and
    every set name is in the app's import list (`detect.rs`).
- `src/zip.rs` unpacks what a real deflater (flate2, a test-only dependency) packs at every level;
  `src/sets.rs` and `src/options.rs` test the recognition rules and the switches.
- `crates/emulator-bridge/tests/arcade_frontend.rs` loads the host build through the engine's own
  loader (`NativeLibretroCore`), the way the app opens a game, by its path: the video program's
  120 frames hash to MAME's, an unknown zip's refusal comes back as the reason word for word, a
  save state round-trips, and the switches are settings the engine knows. Like
  `cboy_frontend.rs` it runs only when given the core:
  `CARCADE_SO=cores/continuum-arcade/target/release/libcontinuum_arcade.so cargo test
  --features emulator-bridge/native-core -p emulator-bridge --test arcade_frontend`.

### The Galaxian family in the app (item I5)

- **Which sets**: `scripts/make-set-list.py` takes them from the GAME() lines of MAME's
  `galaxian.cpp`: the sets on the plain `galaxian_state` class whose machine and init are one of
  ours (galaxian/init_galaxian, mooncrst with init_mooncrst or init_mooncrsu, scramble, theend,
  scobra, frogger, turtles with their own inits), not flagged by MAME as not working, wrong or
  imperfect in colours, sound or graphics, unemulated protection or no cocktail, and whose ROM
  regions are the board's (two sets with extra regions are left out). That gives 119: 47 on
  Galaxian's board, 32 on Moon Cresta's, and 40 on Konami's (16 Scramble, 3 The End, 9 Super
  Cobra, 5 Frogger, 9 Turtles/Amidar). Left out for now: everything else in the driver (Jump Bug,
  Pisces, King & Balloon, the bootlegs with other hardware, ...).
- **Their controls and switches come from MAME**: the script starts MAME on each set with
  zero-filled stand-ins for its ROMs (nothing real is read) and has Lua list every input port's
  fields, so `set_list.rs` knows which bit is which stick direction, button, coin and start, active
  high or low, what the ports read at rest, and whether the stick is four-way. The DIP switches come
  from `-listxml`, worded for players (`SWITCH_WORDS`: Lives, Extra life, Coins, Cabinet,
  Difficulty, ...; service, test and unknown switches stay at the factory setting). Sets with the
  same switches share one settings group (35 groups, keyed `carcade_<first set>_<switch>`), and
  only the loaded game's show. The factory defaults are MAME's, bit for bit (a test holds all 184
  sets to them). Scramble's protection bits (IN2's 20h and 80h) are found in the input definitions.
- **The picture**: the board draws 224 x 768 (three dots a pixel along the raster, for the
  stars); the core averages each three into one and hands out 224 x 256 at 3:4, within the
  224 x 288 the app declares, so the app needed no change.
- **Proved** by `tests/libretro.rs`: each Galaxian and Konami test program zipped as its MAME set
  (galaxian, mooncrst, mooncrstuu, scramble, theend, scobra, frogger, amidar) plays through the
  entry points with the core's frames equal to the board's averaged picture and its sound, the
  board's frames equal to MAME's (before any input changes); Scramble shows exactly its three
  switches; and a unit test in `src/board.rs` checks the controls reach Galaxian's active-high and
  Scramble's active-low bits.

## Every place the app wires it in (item I4, done)

Found by reading every place cBoy and the arcade system appear, and wired the same way.

**Build and packaging**

- `scripts/build-core.sh`: `continuum_arcade` is in `IOS_OPTIONAL_CORES` (the lists region). Its
  config arm (no repo, no pin, `IOS_KIND="cargo-continuum-arcade"`), its build function
  `build_ios_continuum_arcade_core` (cBoy's, for this crate) and its dispatch in `build_ios_core`
  (an `if` block of its own) each sit in a `# recipe-start: continuum_arcade` region. It has its
  own build kind, not cBoy's: `check-core-cache.sh` has a tripwire that only one core uses each.
- **What that costs: nothing for any other emulator.** Since the cache rules of 14 October
  (3651403, 1de468f, d8d8654) the lists aren't a build input and a dispatch may sit in its core's
  region, so adding cArcade moved no other core's fingerprint: `bash scripts/build-core.sh
  ios-fingerprint <core>` gives all 20 existing cores the same fingerprint before and after (fceumm
  `df688ed0e6cd1c83`, mgba `1e0dce5f4510cbde`, ppsspp `e05fced2954ed33e`, continuum_gb
  `e6367c28819ad120`, and so on), and cArcade has one of its own. The first build with it compiles
  only cArcade (about a minute, like cBoy), and so does every build after.
- `scripts/check-core-cache.sh`: the three checks cBoy has (it's never reused from the cache, its
  build function is in its region, only one core uses its kind), and its checks that pin the
  `IOS_OPTIONAL_CORES` line now name it. All 217 checks pass.
- `native/ios/build-engine.sh`: drops a cached `continuum_arcade_libretro_ios.dylib` after the
  cache restore, like cBoy's line: it has no pin, so a cached copy could be from an older commit.
- `native/ios/project.yml`: a four-line embed in the OPTIONAL CORES block, exactly cBoy's shape.
  `package-ipa.sh` drops it when the dylib is missing (it reads `ios-optional-names`).
- `.github/workflows/ios.yml`: parked, see "Parked" below.
- No change: `scripts/extract-core-option-keys.sh` (cBoy's option inventory),
  `scripts/fetch-buildbot-cores.sh` (downloaded cores), and
  `crates/emulator-bridge/src/core_updates.rs`. Like cBoy, cArcade has no pin, so it gets no row
  there and no "this emulator just got an update" line.

**Engine (Rust, `crates/emulator-bridge`)**

- `src/cores/native_core.rs`, `refusal_from_log`: lines starting `cArcade: ` are passed on, with
  a test next to cBoy's.
- `src/launch_failure.rs`, `own_line`, and `uniffi_cores.rs`, `launch_refusal_line`: cArcade's
  refusal is already the sentence a player should read, and its words ("missing a piece") would
  otherwise read as a missing BIOS file, so for this core the line on screen is the core's own
  sentence (unless it gave none). The app asks for it in the `catch` of `ContinuumApp.swift`'s
  `launch(entry:)`. A refused arcade game is never retried on another core: arcade has only one.
- `src/import/detect.rs`: `CARCADE_SETS`, every set name cArcade knows (184 since 16 October), next to `ARCADE_SETS`, so
  each is filed as arcade by its name with no question (some don't pass the chip rule:
  `mspacmanhnc` looks like five .bin ROMs; a test there checks it). A test in the core crate fails
  if one of its sets is missing from that file.
- `src/import/saves.rs`: the arcade arm now says what's true of these boards: they keep no save
  files, so there's nothing to import, and save states work.
- `tests/arcade_frontend.rs`: new, see "How I4 is proved".
- No change: `import/mod.rs` and `uniffi_import.rs` (arcade zips are already kept whole),
  `achievements/console.rs` (arcade is already RetroAchievements' console 27; achievements aren't
  a goal of the first version), `gfx/renderer.rs` (arcade already suggests the scanline look), and
  `bridge.rs`, `battery_layout.rs`, `gfx/screen_layout.rs`, `cores/diagnostic.rs`. Those four are
  cBoy's clock and Super Game Boy crop handling, and cArcade has neither a clock nor a battery
  save.

**App (Swift, `native/ios`)**

- `ContinuumApp.swift`: a `continuumArcade` `CoreSpec` ("cArcade (Arcade)", systems
  `["arcade"]`, 224×288, aspect 3:4, 60.606 fps, 48000 Hz, XRGB8888, no BIOS), in
  `CoreCatalog.all` and as `defaultCoreBySystem[.arcade]`. `unplayableSystems` has arcade only
  when its dylib isn't in the bundle, so the old line stays exactly for a build without it. That
  one table feeds the launch refusal, the Library's "not playable yet" label, the game card's "Runs
  on" (`GameDetailSheet.swift`) and the picker's `canRun` (`ImportCenter.swift`), so they all
  follow. `activePictureAspect` uses the spec's 3:4, right for all three boards; a future
  horizontal board would ask the engine (`corePictureAspect`), as cBoy does.
- `ImportCenter.swift`: the rule that files an arcade zip as Arcade without asking stays; only
  its comments changed. `TouchControls.swift`: the arcade pad (stick, six buttons, COIN on
  Select, START on Start) was already there; only its comments changed.
- Checked here: `swiftc -frontend -parse` on the three files, and the new Swift (the
  `launchRefusalLine` call on a rejected game, the computed `unplayableSystems`, the spec's
  values) typechecked against the engine's freshly generated bindings.
- No change: `SettingsScreen.swift` (a system with one core gets no chooser),
  `GameArtwork.swift` ("FBNeo - Arcade Games" is libretro's thumbnail folder name on their server,
  not a core), `SkinLibrary.swift`, and `CloudSync.swift` (which already syncs the arcade
  guesses).

**Docs**

- `docs/MANIC_PARITY.md`, arcade row: Continuum's own arcade core is in, and what it plays.
- `README.md` (line 30, "Arcade is out for now while we build our own") becomes untrue once
  cArcade ships. It isn't on this job's list, so it's left for whoever writes the notes.

## Parked

Kiro's GitHub rules let no workflow change go straight onto master, and the owner allows no pull
requests, so these two edits to `.github/workflows/ios.yml` wait for whoever can make them.
Until then CI doesn't run the arcade crate's tests, so `cargo test --release` in
`cores/continuum-arcade` must pass locally before every push.

1. In `on.push.paths`, right after `- "cores/continuum-gb/**"`:

   ```yaml
      # cArcade, this project's own arcade emulator. It ships in the .ipa (built from this folder
      # on every run), so a cArcade-only change must start a build like any other.
      - "cores/continuum-arcade/**"
   ```

2. In the `checks` job, right after the "cBoy tests (Game Boy / Color / Advance core)" step:

   ```yaml
         - name: cArcade tests (arcade core)
           # Its own crate (cores/continuum-arcade). The CPU test suites are fetched by its script;
           # without them those tests report SKIPPED and the rest still run.
           run: |
             cores/continuum-arcade/scripts/fetch-test-suites.sh || echo "::warning::test suite download failed"
             cargo test --release --manifest-path cores/continuum-arcade/Cargo.toml
   ```
