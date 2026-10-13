# Night batch A, 13 October 2026: what landed, and what was handed on

Batch A is the small leftovers of the 12 October cleanup (`docs/reports/cleanup-2026-10-12/`) plus
the licence-swap leftovers from STATUS.md "Next up" item 5. Two other agents worked on the same
repository the same night, so anything that could only be fixed inside `cores/continuum-gb/` or
`crates/emulator-bridge/src/peripherals/` is written down here instead of changed.

## Handed on (another agent owns the file)

- **RUST-08, a cBoy refusal showing an unrelated line.** `cores/continuum-gb/src/libretro.rs`
  around line 587 drops `Cart::new`'s reason with `.ok()?`, so the screen shows whatever cBoy
  logged last (an unsupported cartridge with force-colour on is refused as "Game Boy cartridge run
  in Color mode (option)"). The engine side is now plain either way: a refusal the engine cannot
  place is shown as "the file isn't one it can play", and the core's own sentence stays in the
  activity log. cBoy should still log `Cart::new`'s reason and only pass on lines that really are
  refusals.

- **RUST-19, the Amiibo tag id in the tap line.** The hex id is built in
  `crates/emulator-bridge/src/peripherals/amiibo.rs` (`tap`, both the "tapped" and the "not
  tapped" line). It should be dropped: the player tapped a figure they can already see by name,
  and the id means nothing to them. Keep the id in the activity log if it is wanted for support.
- **IOS-12, deciding an Amiibo tap from text.** The same `tap` function returns one sentence and
  the app decides success with `line.contains("tapped")`. The clean fix is a real result from the
  engine rather than a sentence to match.

## Not done tonight, with what the next session needs (A6: Sega CD disc swap and RAM-cart saves)

Both turned out to be bigger than the item looked, and neither can be proved off a phone: a Sega
CD game needs a BIOS file nobody here has, so there is no way to run one through a harness. What
was read out of BlastEm at its pin (`ab74365`), so the next attempt starts informed:

- **Disc swap.** `libblastem.c` registers no disc control interface at all (the engine's side is
  ready: `crates/emulator-bridge/src/cores/disk.rs` takes `SET_DISK_CONTROL_EXT_INTERFACE` and the
  app already has the control for PlayStation). BlastEm has no hook for changing a disc either:
  `system_header` has `lockon_change` for the lock-on cartridge and nothing for media. A swap
  would have to repoint `segacd_context::media` AND `segacd_context::cdd.media` (`cdd_mcu` holds
  its own `system_media *`), then put the drive through tray-open and tray-close (`DS_NO_DISC` in
  `cdd_mcu.h`) so the BIOS re-reads the table of contents. The core would also have to expand an
  `.m3u` itself, since `load_media()` does not know that extension. That is core surgery with no
  way to test it tonight, so nothing was changed: a multi-disc Sega CD game behaves exactly as it
  did.
- **RAM-cart saves.** The cart is `segacd_context::bram_cart` (with `bram_cart_id` and
  `bram_cart_write_enabled`), separate from the 8 KB internal backup RAM at `bram`, which is what
  `RETRO_MEMORY_SAVE_RAM` returns today (deliberately: that layout matches the `.brm` files the
  old core wrote, and `battery_layout.rs` migrates them). libretro has one save region, so the two
  cannot both be it. The two ways out: hand over one buffer holding internal RAM then cart RAM and
  teach `battery_layout.rs` to split it (an old 8 KB save still has to load), or expose the cart
  under its own memory id and store it as a second file. Either needs the save plumbing changed,
  not just the patch.

## What A7 (the "EEPROM carts BlastEm does not know") really turned out to be

The two games named in the item do not save to an EEPROM:

- **Pete Sampras Tennis** (1994) is a J-Cart game with no save at all, and BlastEm's database
  already has both revisions of it, as J-Cart. The Codemasters tennis game that DOES carry an
  EEPROM is the 1996 sequel, *Sampras Tennis 96*, and that one is missing from the database. It
  was not added: the entry needs the EEPROM's size and a dump's sha1, and guessing either would
  give a save that silently does the wrong thing.
- **Super Skidmarks** was missing from the database altogether, so its two extra J-Cart pads
  answered nothing. That entry was added (keyed by the published sha1 of the European cartridge
  and by its product code). No EEPROM: nothing published says that cartridge has one.

Worth knowing for whoever picks up the rest: a J-Cart's two extra pads are players 3 and 4, and
until tonight the libretro glue only ever polled two, so NO J-Cart game could have a third or
fourth player in Continuum. The four-pad poll that came with the multitap work fixes that for
every J-Cart game the database knows (Micro Machines 2, Micro Machines Military, Micro Machines
96, Pete Sampras Tennis and now Super Skidmarks).

## Not done tonight (A9: Mesen 2 as a second SNES choice), and why

Mesen 2 does play SNES games, but everything the Continuum patch
(`scripts/patches/mesen2-nes-memory-cheats-saves.patch`) gave it is written for the NES only, and
that was checked line by line tonight:

- **Memory for RAM search and achievements:** `retro_get_memory_data`,
  `retro_get_memory_size` and the memory map all return early unless the console is
  `ConsoleType::Nes`. On SNES the app would get nothing: no RAM search, no achievements.
- **Cheats:** `retro_cheat_set` returns on the same test, so a typed code would be stored and do
  nothing.
- **In-game saves:** the patch tells Mesen the frontend owns the battery RAM
  (`BatteryManager::SetFrontendOwnsSaveRam(true)`, honoured in `NES/BaseMapper`), and the SNES
  cartridge code never saw that change. With no `RETRO_MEMORY_SAVE_RAM` on SNES either, an SNES
  save would end up in a file of Mesen's own, outside Continuum's saves, so it would not be backed
  up, synced or visible in the app.

So Mesen 2 was NOT offered for SNES. Offering it would have given players a core that cannot
cheat, cannot search RAM, cannot earn achievements, and whose saves quietly leave the app's save
system, which is worse than not offering it. What it needs: the three gates widened to SNES
(`MemoryType::SnesSaveRam` and `SnesWorkRam`, the SNES cheat types, WRAM at `$7E0000` in the
memory map) and the SNES cartridge's own battery load/save skipped the way `BaseMapper`'s is.
Mesen 2 is a REQUIRED core in `scripts/build-core.sh`, so a patch that does not compile fails the
whole build and the owner gets no .ipa: that work wants a session that can build it, not a blind
patch overnight. Speed on an iPhone still needs the phone either way.

## Not done tonight (A10: Continuum's own 32X start-up code), with the groundwork

Nothing changed, so a 32X game still shows the same "needs 32X_M_BIOS.bin and 32X_S_BIOS.bin"
line it did before. What was worked out, so the next attempt does not start from nothing:

- The 68K side is already handled by BlastEm itself. When `32X_G_BIOS.bin` is missing,
  `32x.c get_68K_vector_rom()` builds a vector table of its own (every vector pointing into a
  stub), which is why that file was never a gate in the app.
- The two SH-2 start-up files are simply read into the buffers at `main_map[5]` and `sub_map[5]`
  in `alloc_config_32x()` (around lines 1417 and 1449 at pin `ab74365`), and when the file is
  missing the buffer is left as zeroes, so the SH-2 runs zeroes and the game never starts. Those
  two `else` branches are exactly where code Continuum ships would go, as a byte array compiled
  in rather than a file on disk.
- What that code has to do is small: set up each SH-2 (stack and vector base), tell the 68K it is
  alive through the adapter's communication registers, wait for the handshake the 68K side sends,
  then jump to the start address the cartridge's own 32X header gives for that SH-2.
- Why it stopped here: writing it means hand-building SH-2 machine code with no SH-2 assembler in
  this sandbox and no way to run a 32X game (no Mac, no test cartridge), so there would be no way
  to tell working code from code that silently hangs. It wants a session that can build and run
  BlastEm, where each step can be watched. The licence side is fine: our own code, GPL, in this
  repository.

## Notes for whoever builds next

- **Editing `scripts/build-core.sh` rebuilds every core** (the per-core fingerprint hashes the
  whole script apart from its `IOS_PIN=` lines), so the next build after one pays the long
  45-50 minutes instead of 6-8. Editing one `scripts/patches/<family>-*.patch` rebuilds that core
  only.
