//! continuum-sms (working name): Continuum's own Sega 8-bit core, written from scratch, for the
//! Master System, the Game Gear and the SG-1000. Plan and settings:
//! .kiro/notes/tasks/cboy-finish.json (S1) and .kiro/notes/docs/SMS_OPTIONS_INVENTORY.md.
//!
//! Shown in the app as cGear. Modules: `z80` (cArcade's own Z80, compiled in from its one file),
//! `bus` (the memory map, the cartridge's banking, the I/O ports), `vdp` (the picture chip), `psg`
//! (the sound chip), `sms` (the machine) and `libretro` (the entry points).
//!
//! Done so far: the Z80 on the Master System's real memory map, the Sega mapper with cartridge
//! RAM, the memory control port, the picture chip's ports, registers, memory, line counter,
//! interrupts and counters, the Master System picture (mode 4), the SN76489 sound chip, the pads,
//! and the SDSC debug console that test programs print to. Also the YM2413 FM chip (`fm`, on a Japanese Master System
//! or with the FM setting on), the other mappers, the Game Gear, the SG-1000 and the libretro entry points.

#[path = "../../continuum-arcade/src/z80.rs"]
pub mod z80;

pub mod bus;
pub mod fm;
pub mod libretro;
pub mod link;
pub mod options;
pub mod peripherals;
pub mod psg;
pub mod sms;
pub mod vdp;
