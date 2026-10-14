# cArcade: Continuum's own arcade emulator

**Where it stands (13 October 2026): the Z80 is written and proven (item I2): all 1,604,000
SingleStepTests cases pass, and ZEXDOC and ZEXALL report no errors. No board yet, and nothing of
it is in the app yet.** The write-up at the end of the job finishes this file: the games it plays,
what the tests prove, what is approximate, and what comes next.

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
| `zip.rs` | Reads a zip: its directory (names, sizes, CRC32s), and its files, stored or deflated, with our own inflater and CRC32, so no new dependency |
| `sets.rs` | The built-in set list (short name, title, parent, board, each ROM's size, CRC32 and place) and the recognition rules below |
| `board.rs` | What every board does: run one frame, the picture, the sound, the inputs, the DIP switches, save states |
| `pacman.rs` | Namco's Pac-Man board |
| `namco_wsg.rs` | Pac-Man's sound chip: three voices playing 32-step waves from the sound PROM |
| `galaxian.rs` | Namco's Galaxian board and Konami's Scramble variant |
| `ay8910.rs` | The AY-3-8910, two of them on Konami's sound board (written and proven, see below) |
| `galaxian_sound.rs` | Galaxian's analogue sound, approximated |
| `dkong.rs` | Nintendo's Donkey Kong board |
| `mcs48.rs` | The Intel 8035 that plays Donkey Kong's music into a DAC (written and proven, see below) |
| `dkong_sound.rs` | Donkey Kong's analogue sound effects, approximated |
| `libretro.rs`, `options.rs` | The entry points, and the DIP switches as core options |

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
   "Our arcade emulator can't play this one yet. Right now it plays Pac-Man, Ms. Pac-Man and games
   on the same board." The sentence is built from the boards that are switched on. For a split
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
    in Z80 assembly, plus 8035 assembly for Donkey Kong's sound. They're assembled with an
    assembler built from source in scratch: The Macro Assembler AS (it does both the Z80 and the
    8035), or sjasmplus for the Z80 only.
  - **MAME is the reference.** The same programs run on MAME built locally from source, as a
    subset holding only the needed drivers (MAME's own `SOURCES=` build, `-j3`). It lives in
    scratch and is never committed. It's downloaded as a release tarball, because git in this
    sandbox only reaches this project's repository.
  - **The tests.** MAME's frames and sound for each program become small reference files in the
    crate's `tests/`. The tests compare our picture pixel for pixel. Sound is compared sample for
    sample where the rates match; any tolerance is stated in the test.
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
  - A trace comparison with MAME's core on our own programs, run on MAME's Donkey Kong sound
    board, is the last proof; it waits for the MAME build in scratch.

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
  save state. A comparison with MAME's AY-3-8910 on register writes of our own is the last proof.

## The core as the app sees it

- `valid_extensions` is `zip` (7z is not in the first version). `need_fullpath` is false, so the
  core gets the bytes and the path (see step 3 above).
- The picture is XRGB8888, upright, 224×288 for Pac-Man and 224×256 for the other two, at a 3:4
  aspect, 60.606 fps. Sound is 48 kHz.
- **Save states:** bincode, with the format number carried in `library_version` as cBoy does, so
  the app refuses a state from a different layout. There are no battery saves: these boards have
  no battery memory, and the high score goes when the machine is switched off.
- **DIP switches** are core options with plain labels and factory defaults, keyed
  `carcade_<board>_<switch>`. Examples: lives, bonus life, difficulty, upright or cocktail. The
  app's per-game menu already offers core options for one game.
- **Controls:** Select is coin, Start is the 1-player start, B and A are the first two buttons
  (fire, bomb, jump), and X is the 2-player start. On a four-way stick, a diagonal from the phone's
  pad becomes the newer of its two directions, as the stick's gate would.
- Log lines start with `cArcade: `.

## Every place the app wires it in (item I4)

Found by reading every place cBoy and the arcade system appear.

**Build and packaging**

- `scripts/build-core.sh`:
  - Add `continuum_arcade` to `IOS_OPTIONAL_CORES`.
  - Its config arm (no repo, no pin, `IOS_KIND="cargo-continuum-arcade"`, the dylib name) and its
    build function `build_ios_continuum_arcade_core` each go in a
    `# recipe-start: continuum_arcade` region.
  - One dispatch line in `build_ios_core`, next to cBoy's.
  - It needs its own kind, not cBoy's: `check-core-cache.sh` has a tripwire that only one core
    uses `cargo-continuum-gb`.
- **What that costs, said up front.** The optional-core list, the dispatch line and the region
  markers are all in the part every core shares. That's the cache's own design (393f12c: adding
  a region "rebuilds everything ONCE"). So **the first build with cArcade rebuilds every core
  once, about 45-50 minutes.** Builds after it are the quick kind, with cArcade compiled from the
  checkout each time (about a minute, like cBoy).
  - Checked on a copy of the script: adding the name to the optional list alone changes fceumm's
    fingerprint.
  - Builds 185-187 all built 393f12c, so nothing else forces a full rebuild tonight unless
    another change touches the shared part too.
- `scripts/check-core-cache.sh`: the three checks cBoy has. It's never reused from the cache, its
  build function is in its region, and only one core uses its kind.
- `native/ios/build-engine.sh`: delete a cached `continuum_arcade_libretro_ios.dylib` after the
  cache restore, like cBoy's line. It has no pin, so a cached copy could be from an older commit.
- `native/ios/project.yml`: a four-line embed in the OPTIONAL CORES block, exactly cBoy's shape.
  `package-ipa.sh` drops it when the dylib is missing. It reads `ios-optional-names`, so it needs
  no edit itself.
- `.github/workflows/ios.yml`: add `cores/continuum-arcade/**` to the push paths and a "cArcade
  tests" step after cBoy's in `checks`. Nothing else. The cache key and the .ipa check stay as
  they are, so a build without cArcade isn't flagged there; the app's cores line still names it as
  not in the build.
- No change: `scripts/extract-core-option-keys.sh` (cBoy's option inventory),
  `scripts/fetch-buildbot-cores.sh` (downloaded cores), and
  `crates/emulator-bridge/src/core_updates.rs`. Like cBoy, cArcade has no pin, so it gets no row
  there and no "this emulator just got an update" line.

**Engine (Rust, `crates/emulator-bridge`)**

- `src/cores/native_core.rs`, `refusal_from_log`: also pass lines starting `cArcade: `, with a
  test next to cBoy's.
- `src/launch_failure.rs`: cArcade's refusal is already the sentence a player should read. Its
  words ("missing", "chip") would otherwise file it as a missing BIOS file. So for this core the
  line on screen is the core's own sentence. The app calls this from the `catch` in
  `ContinuumApp.swift`'s `launch(entry:)`. A refused arcade game is never retried on another
  core, because arcade has only one (`launch_retry.rs`).
- `src/import/detect.rs`, `ARCADE_SETS`: add every set name cArcade knows, so each is filed as
  arcade by name with no question. Some of these zips don't pass the chip rule (Galaxian's
  `galmidw.u` and `1h.bin`), and a zip that passes neither rule is asked "Which system?". A test
  in the core crate reads `detect.rs` and fails if any of its set names is missing.
- `src/import/saves.rs`: the arcade arm says arcade games aren't playable. Once the core is in, it
  should say what's true of these boards: they keep no save files, so there's nothing to import,
  and save states work.
- `tests/arcade_frontend.rs`: new, as above.
- No change: `import/mod.rs` and `uniffi_import.rs` (arcade zips are already kept whole),
  `achievements/console.rs` (arcade is already RetroAchievements' console 27; achievements aren't
  a goal of the first version), `gfx/renderer.rs` (arcade already suggests the scanline look), and
  `bridge.rs`, `battery_layout.rs`, `gfx/screen_layout.rs`, `cores/diagnostic.rs`. Those four are
  cBoy's clock and Super Game Boy crop handling, and cArcade has neither a clock nor a battery
  save.

**App (Swift, `native/ios`)**

- `ContinuumApp.swift`:
  - A `continuumArcade` `CoreSpec`: systems `["arcade"]`, 224×288, aspect 3:4, 60.606 fps,
    48000 Hz, XRGB8888, no BIOS. Add it to `CoreCatalog.all`, and make it
    `defaultCoreBySystem[.arcade]`.
  - Keep `unplayableSystems[.arcade]` only for when its dylib isn't in the bundle, so the old line
    stays exactly for a build without the core.
  - That one table already feeds the launch refusal, the Library's "not playable yet" label, the
    game card's "Runs on" (`GameDetailSheet.swift`) and the picker's `canRun`
    (`ImportCenter.swift`), so they all follow.
  - `activePictureAspect` uses the spec's 3:4, which is right for all three boards. A future
    horizontal board would ask the engine (`corePictureAspect`), as cBoy does.
- `ImportCenter.swift`: the rule that files an arcade zip as Arcade without asking, kept as the
  app's guess, stays. Only its comments change.
- `TouchControls.swift`: the arcade pad (stick, six buttons, COIN on Select, START on Start)
  already exists. Only its comment ("never drawn for a game") changes.
- No change: `SettingsScreen.swift` (a system with one core gets no chooser). Also
  `GameArtwork.swift` ("FBNeo - Arcade Games" is libretro's thumbnail folder name on their server,
  not a core), `SkinLibrary.swift`, and `CloudSync.swift` (which already syncs the arcade
  guesses).

**Docs**

- `docs/MANIC_PARITY.md`, arcade row (I4): Continuum's own arcade core is in, and what it plays.
- `README.md` (line 30, "Arcade is out for now while we build our own") becomes untrue once
  cArcade ships. It isn't on this job's list, so it's left for whoever writes the notes.
