# cBoy's cheat codes (29 October 2026)

cBoy reads the same typed cheat codes as mGBA, the Game Boy and Advance emulator the app had before
it, so any cheat list that works on one works on the other. Code: `cores/continuum-gb/src/cheats.rs`
(Game Boy and Game Boy Color) and `cores/continuum-gb/src/gba/cheats.rs` (Advance), through
`retro_cheat_set` in `src/libretro.rs`. The app's cheat screen names the kinds for cBoy
(`crates/emulator-bridge/src/cheats/formats.rs`, the same line as mGBA's). Built, not yet tried on the
phone.

## Game Boy and Game Boy Color

- **Game Genie** (`XXX-XXX`, or `XXX-XXX-XXX` with a compare byte): matched on the address the console
  reads from the cartridge's ROM, as the device does, and with a compare byte only while the ROM's own
  byte there is that one. Codes for addresses past the ROM are refused (as on SameBoy). mGBA writes the
  byte into one ROM bank instead.
- **GameShark** (`TTVVLLHH`): written at the start of every picture's blank. A type other than `01`
  carries a bank: a cartridge RAM bank for $A000-$BFFF, a Color's work RAM bank for $D000-$DFFF, and the
  byte goes into that bank whatever the game has paged in (as the device pages it in for the write).
  mGBA ignores the type, so a banked code lands in whichever bank happens to be in; SameBoy matches the
  bank as here. Writes into cartridge RAM still need the game to have its RAM switched on.
- **CodeBreaker** (`BBAAAA-VV`) and raw `C000:FF`: written like an `01` GameShark code. (mGBA's raw
  reader takes the value from the address's first two digits: here it is the value after the colon.)
- Nothing changes while a real start-up file is running. Codes are for the app's game: with two Game
  Boys on one phone, the second (which may play another game) is left alone.

## Game Boy Advance

GameShark (v1 and v2), Action Replay (v3), CodeBreaker and raw `AAAAAAAA:VV` codes, decrypted and run
the way mGBA does: TEA decryption under each device's seeds, `DEADFACE` reseeding from the devices'
tables, the first line deciding which device a list is for, CodeBreaker's own scrambling after a `9`
line, writes of all three sizes, runs that step the address and value, adds, ANDs, ORs, pointers,
conditions with Action Replay's ELSE and ENDIF blocks, ROM patches (seen wherever the cartridge is
read, mirrors included), and hook codes (the codes then run when the processor reaches the hook's
address, instead of once a frame at the blank). Not done, as on mGBA: the slowdown code, Action Replay
codes that switch others off, and codes that wait for the device's own button.

Where cBoy reads a list differently from mGBA, on purpose (each is a mistake in mGBA that writes
something the list never asked for, or loses a line): a GameShark game ID line writes nothing (mGBA
writes $001DC0DE wherever its first word points); a "one word to many addresses" code writes only to
the addresses that follow (mGBA also writes the word at an address equal to the word); a list of none
or one address doesn't swallow the next line; a refused Action Replay line leaves nothing half made;
an Action Replay block with no ENDIF runs to the end of the list (mGBA's guards nothing); a new block
ends the one before it; a ROM patch whose value line never comes, and a CodeBreaker list cut short or
of none, write nothing they weren't given (mGBA uses whatever its memory held).

## How it was checked

- `tests/cheats_e2e.rs`: a small Game Boy game and a small Advance game through the libretro entry
  points, as the app drives them: Game Genie with and without a compare byte, GameShark, CodeBreaker,
  raw and joined codes on the Game Boy; a ROM patch, plain and encrypted GameShark, encrypted Action
  Replay (its lines as two cheats) and CodeBreaker on the Advance; on, off, all off, through a save
  state and a restart.
- Unit tests in both files: decoding, banks, the compare byte, TEA both ways, which device a list is
  for, an Action Replay block with ELSE, a CodeBreaker list, ROM patches through the bus, and a hook
  that runs the codes before the first blank.
- `tests/gba_cheats_mgba.rs`: against mGBA itself. `.kiro/notes/wip/tools/mgba-cheats/build.sh` builds
  mGBA's own cheat code (the commit the app ships, 7a12d6d) around a fake memory. The test makes 36,000
  lists of every kind (encrypted and plain, reseeds, hooks, ROM patches, fills, conditions, closed
  Action Replay blocks, random lines), and both must find the same hook, patch the same ROM bytes and
  write the same bytes when the program runs twice. On 29 October: **23,075 lists alike** (234,543
  bytes written or patched between them), none different; the rest met one of the differences above
  and were counted apart, by kind.
