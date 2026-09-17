#!/usr/bin/env python3
"""Builds the Master System test cartridge used to verify the genesis_plus_gx core.

Why a Master System ROM and not a Mega Drive one: Genesis Plus GX emulates both, and
the Z80 side needs a tenth of the setup code the 68000 side does (no vector table, no
TMSS handshake, no bus arbitration). The point of this cart is to prove the *core*
runs — CPU, VDP, PSG and controller ports — and SMS mode exercises all four through
the same libretro entry points.

The cart is written to be asserted against by scripts/core-abi-test.mjs:

  * it fills the screen with a bordered tile so a test can tell "the picture was
    rendered" from "a flat fill was uploaded";
  * its idle colour is magenta and its pressed colour is cyan, both distinct from the
    NES cart (green/red) and the GBA cart (blue/yellow), so a hot-swap test can name
    which core drew a frame from the pixels alone;
  * holding right scrolls the playfield one pixel per frame, which only works if
    per-frame register writes are reaching the VDP;
  * channel 0 of the PSG holds a ~440 Hz tone so the audio path is never silent.

Nothing here is a commercial ROM: it is a few hundred hand-assembled bytes.

    python3 scripts/make-sms-rom.py     # -> web/roms/sms-testcart.sms
"""

import pathlib
import sys

ROM_SIZE = 0x8000  # 32 KB: the smallest size that needs no mapper configuration.

# --- ports -------------------------------------------------------------------------
PSG = 0x7F
VDP_DATA = 0xBE
VDP_CTRL = 0xBF
IO_PORT_A = 0xDC  # player 1: bit4 = button 1, bit5 = button 2, bits0-3 = d-pad

# --- colours (SMS CRAM is --BBGGRR, two bits per channel) --------------------------
BLACK = 0x00
MAGENTA = 0x33  # R=3, G=0, B=3
CYAN = 0x3C  # R=0, G=3, B=3

SCROLL_X = 0xC000  # one byte of work RAM


class Z80:
    """A deliberately small Z80 assembler: only the opcodes this cart uses.

    Labels are resolved in a second pass, so forward jumps read naturally. Every
    emitter asserts its operand range — a silently truncated immediate in hand-written
    machine code is a genuinely awful thing to debug.
    """

    def __init__(self, origin=0x0000):
        self.origin = origin
        self.code = bytearray()
        self.labels = {}
        self.fixups = []  # (offset, label, kind) where kind is 'abs16' or 'rel8'

    # -- plumbing --
    def emit(self, *values):
        for value in values:
            assert 0 <= value <= 0xFF, f"byte out of range: {value}"
            self.code.append(value)
        return self

    def label(self, name):
        assert name not in self.labels, f"duplicate label {name}"
        self.labels[name] = self.origin + len(self.code)
        return self

    def _abs16(self, label):
        self.fixups.append((len(self.code), label, "abs16"))
        self.emit(0, 0)

    def _rel8(self, label):
        self.fixups.append((len(self.code), label, "rel8"))
        self.emit(0)

    def _imm16(self, value):
        assert 0 <= value <= 0xFFFF, f"word out of range: {value}"
        self.emit(value & 0xFF, (value >> 8) & 0xFF)

    def resolve(self):
        for offset, name, kind in self.fixups:
            assert name in self.labels, f"undefined label {name}"
            target = self.labels[name]
            if kind == "abs16":
                self.code[offset] = target & 0xFF
                self.code[offset + 1] = (target >> 8) & 0xFF
            else:
                delta = target - (self.origin + offset + 1)
                assert -128 <= delta <= 127, f"jr to {name} out of range ({delta})"
                self.code[offset] = delta & 0xFF
        return bytes(self.code)

    # -- instructions --
    def di(self):
        return self.emit(0xF3)

    def im1(self):
        return self.emit(0xED, 0x56)

    def retn(self):
        return self.emit(0xED, 0x45)

    def ld_sp(self, value):
        self.emit(0x31)
        self._imm16(value)
        return self

    def ld_a(self, value):
        return self.emit(0x3E, value)

    def ld_b(self, value):
        return self.emit(0x06, value)

    def ld_hl(self, label):
        self.emit(0x21)
        self._abs16(label)
        return self

    def ld_bc(self, value):
        self.emit(0x01)
        self._imm16(value)
        return self

    def ld_a_from(self, addr):
        self.emit(0x3A)
        self._imm16(addr)
        return self

    def ld_to_a(self, addr):
        self.emit(0x32)
        self._imm16(addr)
        return self

    def ld_a_hl(self):
        return self.emit(0x7E)

    def ld_a_b(self):
        return self.emit(0x78)

    def ld_b_a(self):
        return self.emit(0x47)

    def inc_hl(self):
        return self.emit(0x23)

    def inc_a(self):
        return self.emit(0x3C)

    def dec_bc(self):
        return self.emit(0x0B)

    def or_c(self):
        return self.emit(0xB1)

    def xor_a(self):
        return self.emit(0xAF)

    def and_n(self, value):
        return self.emit(0xE6, value)

    def cp_n(self, value):
        return self.emit(0xFE, value)

    def out_n(self, port):
        return self.emit(0xD3, port)

    def in_n(self, port):
        return self.emit(0xDB, port)

    def djnz(self, label):
        self.emit(0x10)
        self._rel8(label)
        return self

    def jr(self, label):
        self.emit(0x18)
        self._rel8(label)
        return self

    def jr_z(self, label):
        self.emit(0x28)
        self._rel8(label)
        return self

    def jr_nz(self, label):
        self.emit(0x20)
        self._rel8(label)
        return self

    def jp(self, label):
        self.emit(0xC3)
        self._abs16(label)
        return self

    def jp_addr(self, addr):
        self.emit(0xC3)
        self._imm16(addr)
        return self

    # -- VDP helpers --
    def vdp_reg(self, reg, value):
        """Register write: data byte first, then 0x80 | register number."""
        self.ld_a(value)
        self.out_n(VDP_CTRL)
        self.ld_a(0x80 | reg)
        self.out_n(VDP_CTRL)
        return self

    def vdp_address(self, addr, mode):
        """Point the VDP at `addr`. mode 0x40 = VRAM write, 0xC0 = CRAM write."""
        self.ld_a(addr & 0xFF)
        self.out_n(VDP_CTRL)
        self.ld_a(mode | ((addr >> 8) & 0x3F))
        self.out_n(VDP_CTRL)
        return self


def tile_data():
    """Two 8x8 tiles, 4 bitplanes, row-interleaved: 4 bytes per row, 32 per tile.

    Tile 0 is blank. Tile 1 is a 6x6 block of colour 1 inside a colour-0 border, so a
    screen tiled with it contains both dark and bright pixels at a predictable ratio
    (28 dark, 36 bright per tile). A flat fill would pass a "did it draw" check; this
    does not.
    """
    blank_row = bytes([0x00, 0x00, 0x00, 0x00])
    # Bit 7 of each plane byte is the leftmost pixel, so 0x7E lights columns 1..6.
    inner_row = bytes([0x7E, 0x00, 0x00, 0x00])
    tile0 = blank_row * 8
    tile1 = blank_row + inner_row * 6 + blank_row
    return tile0 + tile1


# The Z80 reserves the bottom of the address space for vectors: RST 0x00-0x38, the
# maskable interrupt at 0x38 and the NMI (the SMS pause button) at 0x66. The main
# program therefore starts above all of them, and 0x0000 holds only a jump.
#
# Getting this wrong is not a compile error, it is a silently corrupted program: the
# first version of this cart laid 321 bytes of code across 0x0000 and then wrote the
# NMI handler on top, replacing two bytes in the middle of the tile-upload loop. The
# PSG still played, so the cart looked alive while rendering an entirely black screen.
PROGRAM_ORIGIN = 0x0070


def build_program():
    a = Z80(origin=PROGRAM_ORIGIN)

    a.ld_sp(0xDFF0)

    # VDP registers. R1 keeps the display off until VRAM holds something worth
    # showing. Frame interrupts stay disabled (R1 bit 5 clear) so no handler is needed.
    a.vdp_reg(0x00, 0x04)  # mode 4
    a.vdp_reg(0x01, 0x00)  # display off
    a.vdp_reg(0x02, 0xFF)  # name table  -> 0x3800
    a.vdp_reg(0x03, 0xFF)  # unused in mode 4, must be 0xFF
    a.vdp_reg(0x04, 0xFF)  # unused in mode 4, must be 0xFF
    a.vdp_reg(0x05, 0xFF)  # sprite attribute table -> 0x3F00
    a.vdp_reg(0x06, 0xFB)  # sprite patterns -> 0x0000
    a.vdp_reg(0x07, 0x00)  # backdrop colour -> CRAM[16]
    a.vdp_reg(0x08, 0x00)  # scroll X
    a.vdp_reg(0x09, 0x00)  # scroll Y
    a.vdp_reg(0x0A, 0xFF)  # line counter: no line interrupts

    # ---- palette ----
    a.vdp_address(0x0000, 0xC0)
    a.ld_a(BLACK)
    a.out_n(VDP_DATA)  # CRAM[0]: the tile border
    a.ld_a(MAGENTA)
    a.out_n(VDP_DATA)  # CRAM[1]: toggled by the buttons
    a.vdp_address(0x0010, 0xC0)
    a.ld_a(BLACK)
    a.out_n(VDP_DATA)  # CRAM[16]: the backdrop R7 points at

    # ---- tiles ----
    a.vdp_address(0x0000, 0x40)
    a.ld_hl("tiles")
    a.ld_b(64)  # two tiles
    a.label("tile_loop")
    a.ld_a_hl()
    a.out_n(VDP_DATA)
    a.inc_hl()
    a.djnz("tile_loop")

    # ---- name table: 32 x 28 entries of tile 1 ----
    # Writes to the data port auto-increment the VRAM pointer, so the address is set
    # once. Each entry is two bytes: tile index low, then flags (all clear).
    a.vdp_address(0x3800, 0x40)
    a.ld_bc(32 * 28)
    a.label("nt_loop")
    a.ld_a(0x01)
    a.out_n(VDP_DATA)
    a.xor_a()
    a.out_n(VDP_DATA)
    a.dec_bc()
    a.ld_a_b()
    a.or_c()
    a.jr_nz("nt_loop")

    # ---- PSG: a ~440 Hz square on channel 0, the rest muted ----
    # tone = 3579545 / (32 * n); n = 254 gives 440.4 Hz.
    a.ld_a(0x8E)  # latch ch0 tone, low nibble of 254
    a.out_n(PSG)
    a.ld_a(0x0F)  # high six bits of 254
    a.out_n(PSG)
    a.ld_a(0x90)  # ch0 volume, attenuation 0
    a.out_n(PSG)
    for mute in (0xBF, 0xDF, 0xFF):  # channels 1-3 fully attenuated
        a.ld_a(mute)
        a.out_n(PSG)

    a.xor_a()
    a.ld_to_a(SCROLL_X)

    a.vdp_reg(0x01, 0x40)  # display on

    # ---- main loop ----
    a.label("frame")

    # Wait for vblank by polling the status port. Reading it clears the flag, which is
    # why this is a valid edge detect without interrupts.
    a.label("vblank_wait")
    a.in_n(VDP_CTRL)
    a.and_n(0x80)
    a.jr_z("vblank_wait")

    a.in_n(IO_PORT_A)

    # Buttons are active low. Either button 1 (bit 4) or button 2 (bit 5) turns the
    # playfield cyan, so the cart responds whichever way a frontend maps its pad.
    a.and_n(0x30)
    a.cp_n(0x30)
    a.jr_z("idle_colour")
    a.ld_a(CYAN)
    a.jr("write_colour")
    a.label("idle_colour")
    a.ld_a(MAGENTA)
    a.label("write_colour")
    # CRAM[1] = A. The address must be set every frame because the data write moved it.
    a.ld_b_a()  # stash the colour; vdp_address clobbers A
    a.ld_a(0x01)
    a.out_n(VDP_CTRL)
    a.ld_a(0xC0)
    a.out_n(VDP_CTRL)
    a.ld_a_b()
    a.out_n(VDP_DATA)

    # Right (bit 3, active low) scrolls one pixel per frame.
    a.in_n(IO_PORT_A)
    a.and_n(0x08)
    a.jr_nz("no_scroll")
    a.ld_a_from(SCROLL_X)
    a.inc_a()
    a.ld_to_a(SCROLL_X)
    a.label("no_scroll")
    a.ld_a_from(SCROLL_X)
    a.out_n(VDP_CTRL)
    a.ld_a(0x88)  # register 8: scroll X
    a.out_n(VDP_CTRL)

    a.jp("frame")

    a.label("tiles")
    a.emit(*tile_data())

    return a


def main():
    program = build_program()
    code = program.resolve()

    rom = bytearray(b"\xff" * ROM_SIZE)

    # Reset vector: mask interrupts before anything else, then jump clear of the
    # vector table.
    boot = Z80()
    boot.di()
    boot.im1()
    boot.jp_addr(PROGRAM_ORIGIN)
    boot_code = boot.resolve()
    rom[0 : len(boot_code)] = boot_code

    # Interrupts stay masked, but a stray one landing on 0xFF bytes would be
    # untraceable, so both handlers return immediately.
    irq = Z80()
    irq.emit(0xC9)  # ret
    rom[0x0038:0x0039] = irq.resolve()

    nmi = Z80()
    nmi.retn()
    rom[0x0066:0x0068] = nmi.resolve()

    assert len(boot_code) <= 0x38, "boot stub runs into the interrupt vector"
    end = PROGRAM_ORIGIN + len(code)
    assert end <= 0x7FF0, f"program overruns the header at 0x7FF0 (ends 0x{end:04X})"
    rom[PROGRAM_ORIGIN:end] = code

    # Standard SMS header. Genesis Plus GX identifies the system from the file
    # extension, but the header is what region detection reads, and an absent one
    # leaves that to chance.
    rom[0x7FF0:0x7FF8] = b"TMR SEGA"
    rom[0x7FF8:0x7FFA] = b"\x00\x00"
    rom[0x7FFC:0x7FFF] = b"\x00\x00\x00"  # product code + version
    rom[0x7FFF] = 0x4C  # export SMS, 32 KB

    checksum = sum(rom[0:0x7FF0]) & 0xFFFF
    rom[0x7FFA] = checksum & 0xFF
    rom[0x7FFB] = (checksum >> 8) & 0xFF

    out = pathlib.Path(__file__).resolve().parent.parent / "web" / "roms" / "sms-testcart.sms"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(bytes(rom))

    print(f"wrote {out.relative_to(out.parent.parent.parent)} ({len(rom)} bytes)")
    print(
        f"  program {len(code)} bytes at 0x{PROGRAM_ORIGIN:04X}-0x{end - 1:04X}, "
        f"tile data at 0x{program.labels['tiles']:04X}"
    )
    print(f"  idle magenta 0x{MAGENTA:02X}, pressed cyan 0x{CYAN:02X}, checksum 0x{checksum:04X}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
