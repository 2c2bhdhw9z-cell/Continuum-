# cMini: Continuum's Pokémon Mini emulator

cMini is the standalone Rust crate `cores/continuum-mini`, core ID `continuum_mini`, library
`continuum_mini_libretro_ios.dylib`, setting prefix `cmini_`. It is Continuum's own emulator. Its
yardstick is PokeMini, the emulator it replaces (libretro's PokeMini is the app's default
Pokémon Mini core and stays first until cMini has been played on the phone).

## The machine (29 October 2026)

Every part follows what PokeMini does, PokeMini's timings having been measured on the console by
Team Pokeme. Nothing of PokeMini's code is in cMini; PokeMini was read for behaviour and is run
next to cMini in the tests.

- `minx.rs`: the MINX processor, all 768 opcodes with their cycles, U's one-instruction delay, and
  the points where it asks the interrupt controller again (F's level or U changing).
- `irq.rs`: the interrupt controller, its fixed order of sources, the one accepted interrupt
  waiting for the processor.
- `timers.rs`: the seconds counter, the 256 Hz timer and its 32/8/2/1 Hz interrupts, the three
  programmable timers on both oscillators, 8- and 16-bit, timer 3's pivot.
- `prc.rs`: the picture chip, its 855-cycle steps, drawing at $18, copying at $39, the frame's end at
  $42, the rate divider, the processor waiting while it works, the background map and 24 sprites.
- `lcd.rs`: the LCD controller (its 9 pages, every command, the contrast and its levels) and the
  panel: analog fading over four frames, three shades or two.
- `ports.rs`: the buttons and their interrupts, the shock sensor, the battery sensor, the I/O port,
  the save chip's two-wire protocol (8 KiB), the motor.
- `audio.rs`: timer 3's pulse sampled at 44100 Hz as PokeMini's "direct PWM", through PokeMini's
  piezo model; the libretro low-pass softening.
- `machine.rs`: the memory map and the clocking: the processor runs at least 8 cycles (or gives 32
  to a working picture chip), then everything catches up, as PokeMini's libretro core runs it.
- `bios.rs`: cMini's own start-up program (below).
- `libretro.rs`: 96×64 at 72 Hz (60 Hz runs six frames to every five), stereo sound, the same
  buttons and turbo as PokeMini's core, the screen shake and rumble from the motor, the clock set
  from the phone's local time at every start, reset and loaded state, 8 KiB save RAM (the same
  payload as PokeMini's `.eep`, which the app imports), 8 KiB system RAM laid out as PokeMini's
  (RAM, then the I/O registers) for achievements, states of format 2 without the cartridge in them.

## The start-up program

With `cmini_bios` off, or with no whole `bios.min`, cMini runs its own start-up program, written
for cMini: it sets the console up, clears RAM and the LCD and enters the game at $2102 with exactly
the state PokeMini's free program hands over (every register, every I/O register but the picture
chip's frame count, RAM, the LCD's memory). Interrupts go to the cartridge's table or to a handler
the game put in RAM, and the services games call with CINT (suspend, shut down, the contrast,
the LCD, the RAM handler, the cartridge's power, the picture chip's rate, a pulse, the structure
test) give the same answers. With `cmini_bios` on and the player's `bios.min`, that runs instead.

## How it is proved

`tests/console_pokemini.rs` runs PokeMini itself (`pmharness`, built by
`.kiro/notes/wip/tools/pokemini/build.sh`, kept in scratch) next to cMini, both started as
PokeMini's libretro core starts:

- instruction by instruction: registers, cycles and the processor's state at every step;
- frame by frame with buttons and the shake: RAM, the I/O registers, the LCD's memory, the panel's
  shades, every sound sample and every pixel at every frame;
- over PokeMini's five GPL example programs and 24 made-up programs that write and read every
  register at random, draw with the picture chip, talk to the save chip and take interrupts (a
  bug planted in the sprites, a timer's prescaler, the piezo model or the save chip is caught);
- the start-up program's hand-over and its services against PokeMini's free one.

On 29 October all agree: 300,000 instructions and 1,200 frames per program. `tests/cpu_pokemini.rs`
checks every opcode on 42,768 random states, interrupt check points included.
`tests/libretro.rs` drives the core as the app does (frames, sound counts, memory sizes, a state
played back exactly, reset, 60 Hz). Run:

    bash .kiro/notes/wip/tools/pokemini/build.sh
    P=/projects/sandbox/.scratch/pokemini; S=$P/PokeMini-f8eb158f7082d80739075baa3653afc14fdf3d70
    POKEMINI_CPU_HARNESS=$P/cpuharness POKEMINI_HARNESS=$P/pmharness \
      POKEMINI_BIOS=$S/freebios/freebios.min POKEMINI_PROGRAMS=$S/dev-misc/examples \
      cargo test --release --manifest-path cores/continuum-mini/Cargo.toml

## Where cMini does what PokeMini meant, not what it does

- $2053-$2055 are three registers. PokeMini's writes to them run on into the I/O direction
  register's (missing `return`s); MAME keeps them apart. The test build of PokeMini is corrected
  the same way.
- The colour add-on's address and the LCD's column stay inside their memories (PokeMini's can
  run past them).
- The sound of each frame goes out with that frame. PokeMini's buffer holds it back about 80 ms.
- The start-up program's suspend gives back the interrupt priorities it changed (PokeMini's clears
  $2021), and its structure test returns properly (PokeMini's leaves a byte on the stack, so its
  return lands in the start-up program and the game starts over).
- With the player's `bios.min`, cMini starts at that program's own reset address. PokeMini's
  libretro core takes the address from its free program and runs the player's from there.

## Known gaps

- PokeMini holds the power button for up to 72 frames when a game closes, so a game that saves on
  power-off can save. In Continuum the app saves the save memory before the core is closed, so
  the core can't do that on its own.
- The infrared setting connects the console's infrared lines, but no two-phone transport carries
  them yet; with nothing connected the receiver reads dark, as on PokeMini (which has no infrared).
- No colour games (neither has PokeMini's libretro core).
- Not yet played on a phone: Built, untested.
