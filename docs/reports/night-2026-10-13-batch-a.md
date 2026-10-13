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
- **CBOY-06, the Super Game Boy border switched on while a game is running.** The app no longer
  works the picture's shape out for itself: it asks the engine, and the engine answers with what
  cBoy's own `retro_get_system_av_info` says (A20). That covers the start of a game, including
  "Auto (Super Game Boy)", which the settings alone could never tell. What is left is cBoy's side
  of a live change: when `cboy_sgb_border` moves mid-game it should tell the frontend with
  `RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO` (65), the way FCEUmm does when its region option moves.
  The engine already picks that up on the next frame (`sync_descriptor_from_core`), so one env
  call in `cores/continuum-gb` is the whole fix and nothing on the app side has to change.

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

## A16: what the Gearsystem "16 bytes" were, and the fix

The finding was right down to the number. Reproduced off the phone: Gearsystem built from source at
its pin (`692494c8`), two processes standing in for two phones, one playing straight through to
frame 210, the other joining at frame 50 and loading the first one's frame-200 state and playing on
to 210. The same moment in the same game, and the two states differed in exactly 16 bytes, every
time, on a Master System cartridge, a Game Gear one and a bank-switched one.

**The 16 bytes are the Sports Pad's timing block, eight in each of the two controller ports.**
`Input::stSportsPad::last_clock` holds `GearsystemCore::m_master_clock_cycles`, the count of cycles
since this phone opened the game. That counter is not in the save state (only the link-cable state
carries it), it is zeroed by `Reset()`, and `Input::LoadState` throws the loaded value away and
re-anchors it to the loading machine's own counter — so the core already treats it as local
bookkeeping, while still writing it into every state. Two phones that reach the same frame by
different routes have different counters, so the hash online play compares never matched. The
re-anchoring also shifted what the pad had piled up (`unknown_cycles`, `absolute_phase`,
`absolute_phase_cycles`), because `Input::SaveState` calls `SyncSportsPad` and so how the elapsed
time was chopped up depended on when the frontend happened to save.

Two hunks in `scripts/patches/gearsystem-sg1000-memory-map.patch` fix it:

- `Input::SaveState` writes `last_clock` as cycles-since-now instead of the running total (zero in
  practice, since `SyncSportsPad` has just caught both pads up), then puts the live value back so
  saving a state does not disturb the game. This is the same shape as the gear-to-gear timing
  upstream already saves as `tx_delta`/`rx_delta`.
- `SyncSportsPad` counts nothing for a pad that is not plugged in: it keeps the anchor current and
  returns. Nothing a player sees changes — `EnableSportsPad` calls `ResetSportsPad` when a pad is
  plugged in, so the piled-up time was never read — and Continuum never plugs one in at all
  (nothing in the engine sets `RETRO_DEVICE_SPORTS_PAD`; the Sports Pad is not offered).

**A second, smaller fault found while looking: ten bytes of leftovers from memory.** Four structures
are written into the state whole, and the spare bytes the compiler adds inside them were never
written, so each state carried whatever happened to be in that memory: `Video::Phaser` (2 bytes),
`Input::stPaddle` (2), `Input::stSportsPad` (3 each, so 6), and `Eeprom93C46MemoryRule::EEPROM93C46`
(1, only on the two EEPROM cartridges). Proved by dirtying the heap and the stack with two different
byte patterns before loading the core: the phaser's two bytes came out as the pattern. The patch
names those spare bytes in the structures — same size, same field offsets, so the state format does
not move — and zeroes them before each write.

Checked off the phone, all with the whole Continuum patch applied to a clean tree at the pin:

- two phones, same frame, two routes: 16 bytes differed before, 0 after (Master System, Game Gear,
  bank-switched cartridge);
- two different heap and stack patterns: 2 bytes differed before, 0 after;
- the state is the same size as before, byte for byte in layout: a state written by the shipped
  build loads in the patched one and a state written by the patched build loads in the shipped one,
  so nobody's save states are lost;
- save, load it back, save again: byte-identical, which it was not before.

No Rust test came with it: the fix is inside the C++ core, which the host test suite cannot load, so
the check that matters is the harness above rather than a unit test that could only re-test the
engine's hash function (`netplay::state_hash` is already covered). The harness is four small files
(a `dlopen` runner, a 32 KB test cartridge generator, a byte comparison and a heap/stack poisoner)
and lives outside the repository in the night's scratch folder.

Still open for whoever wants it, and not a Continuum problem today: with a Sports Pad actually
plugged in, saving a state still chops the pad's elapsed time at the moment of the save, so two
phones using that peripheral in online play could still drift. Fixing that means carrying the
pending cycles in the state and not syncing at save time, which changes what the field means and so
needs the save-state version gate upstream uses (`if (version >= 108)`).

## Notes for whoever builds next

- **Editing `scripts/build-core.sh` rebuilds every core** (the per-core fingerprint hashes the
  whole script apart from its `IOS_PIN=` lines), so the next build after one pays the long
  45-50 minutes instead of 6-8. Editing one `scripts/patches/<family>-*.patch` rebuilds that core
  only.
- Because of that, the save-state fix above went into the existing
  `gearsystem-sg1000-memory-map.patch` rather than a patch file of its own, which would have meant
  editing `build-core.sh` and rebuilding all 33 cores. Its name now undersells it: next time
  `build-core.sh` is edited for some other reason, its gearsystem comment should say the patch
  carries the SG-1000 memory map **and** the save-state fix.
- **That happened in A23, so tonight's build is the long one.** Pinning the CMake and ninja
  downloads (RUST-16) could only be done in `build-core.sh`, so every from-source core rebuilds
  once on the next build and it costs the 45-50 minutes rather than 6-8. The gearsystem comment
  above was corrected in the same edit, as this note asked.

## Left alone on purpose (A23)

- **The "Beetle has no HLE" line in the launch message cannot be reached.** `ContinuumApp.swift`
  has a PlayStation branch telling the player Beetle PSX HW needs a real BIOS, but it only runs
  when `missingFirmware(for:)` returns something, and since `876fe64` nothing puts PlayStation in
  either firmware table (`requiredFirmware` has no `.ps1` entry, `requiredCoreFirmware` only
  Saturn). So no player can see that line, and its wording is stale: with no BIOS file Beetle
  boots its own built-in OpenBIOS. The comments around it were corrected; the line itself is text
  a tester would read, so changing or dropping it is the owner's call, not a cleanup edit. Whoever
  picks it up should delete the dead branch rather than reword it.
