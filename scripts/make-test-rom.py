#!/usr/bin/env python3
"""
Generates web/roms/nes-testcart.nes — an original NES test ROM for this project.

Why this exists: verifying "ROM → core → bridge → WebGPU → screen" needs a ROM, and
commercial ROMs cannot be shipped. This one is written here from scratch (6502
assembled by the tiny assembler below), so it is ours to distribute, and it is
*designed* to be asserted against:

  - It fills the screen with a bordered-square tile pattern, which only a working
    PPU emulation, a correct RGB565 frame path and a correct blit will produce.
  - The whole screen is one background palette, so a test can assert "mostly green".
  - Holding A repaints that palette entry red, giving a single, unambiguous signal
    that input reached the core: green → red.
  - Left/Right scroll the background, proving per-frame register writes take effect.
    Note the pattern's border rows are invariant under horizontal scroll, so a test
    must sample a fill row (y % 8 in 1..6), not a border row.
  - Pulse channel 1 plays a continuous ~440 Hz square wave, so silence in the output
    means the audio path dropped it rather than the ROM never producing any.

Mapper 0 (NROM), 16 KB PRG, 8 KB CHR, NTSC.
"""

import pathlib
import sys

# --------------------------------------------------------------------- assembler

# Only the opcodes this program needs. An incomplete table that fails loudly beats a
# full one that silently assembles the wrong addressing mode.
IMPLIED = {
    "sei": 0x78, "cld": 0xD8, "txs": 0x9A, "inx": 0xE8, "iny": 0xC8,
    "dex": 0xCA, "dey": 0x88, "tax": 0xAA, "tay": 0xA8, "txa": 0x8A,
    "tya": 0x98, "lsr": 0x4A, "clc": 0x18, "sec": 0x38, "rts": 0x60,
    "pha": 0x48, "pla": 0x68, "nop": 0xEA, "rti": 0x40,
}
IMMEDIATE = {"lda": 0xA9, "ldx": 0xA2, "ldy": 0xA0, "cmp": 0xC9, "cpx": 0xE0,
             "cpy": 0xC0, "and": 0x29, "ora": 0x09, "eor": 0x49, "adc": 0x69,
             "sbc": 0xE9}
ZEROPAGE = {"lda": 0xA5, "sta": 0x85, "ldx": 0xA6, "stx": 0x86, "ldy": 0xA4,
            "sty": 0x84, "inc": 0xE6, "dec": 0xC6, "rol": 0x26, "eor": 0x45,
            "and": 0x25, "cmp": 0xC5}
ABSOLUTE = {"lda": 0xAD, "sta": 0x8D, "ldx": 0xAE, "stx": 0x8E, "ldy": 0xAC,
            "sty": 0x8C, "bit": 0x2C, "jmp": 0x4C, "jsr": 0x20, "inc": 0xEE}
ABSOLUTE_X = {"lda": 0xBD, "sta": 0x9D}
BRANCHES = {"bpl": 0x10, "bmi": 0x30, "bne": 0xD0, "beq": 0xF0, "bcc": 0x90,
            "bcs": 0xB0}


class Assembler:
    """Two-pass absolute assembler. Pass 1 sizes instructions and records labels."""

    def __init__(self, origin):
        self.origin = origin
        self.labels = {}
        self.lines = []

    def label(self, name):
        self.lines.append(("label", name))

    def op(self, mnemonic, operand=None, mode=None):
        self.lines.append(("op", mnemonic.lower(), operand, mode))

    def bytes(self, data):
        self.lines.append(("bytes", bytes(data)))

    def _size(self, kind, *rest):
        if kind == "label":
            return 0
        if kind == "bytes":
            return len(rest[0])
        mnemonic, operand, mode = rest
        if operand is None:
            return 1
        if mnemonic in BRANCHES:
            return 2
        if mode == "imm" or mode == "zp":
            return 2
        return 3

    def assemble(self):
        # Pass 1: addresses.
        pc = self.origin
        for line in self.lines:
            if line[0] == "label":
                self.labels[line[1]] = pc
            else:
                pc += self._size(*line)

        # Pass 2: encode.
        out = bytearray()
        pc = self.origin
        for line in self.lines:
            if line[0] == "label":
                continue
            if line[0] == "bytes":
                out += line[1]
                pc += len(line[1])
                continue

            _, mnemonic, operand, mode = line
            if operand is None:
                if mnemonic not in IMPLIED:
                    raise ValueError(f"no implied form for '{mnemonic}'")
                out.append(IMPLIED[mnemonic])
                pc += 1
                continue

            value = self.labels[operand] if isinstance(operand, str) else operand

            if mnemonic in BRANCHES:
                offset = value - (pc + 2)
                if not -128 <= offset <= 127:
                    raise ValueError(f"branch out of range: {mnemonic} -> {operand}")
                out += bytes([BRANCHES[mnemonic], offset & 0xFF])
                pc += 2
                continue

            if mode == "imm":
                out += bytes([IMMEDIATE[mnemonic], value & 0xFF])
                pc += 2
            elif mode == "zp":
                out += bytes([ZEROPAGE[mnemonic], value & 0xFF])
                pc += 2
            elif mode == "absx":
                out += bytes([ABSOLUTE_X[mnemonic], value & 0xFF, (value >> 8) & 0xFF])
                pc += 3
            else:
                out += bytes([ABSOLUTE[mnemonic], value & 0xFF, (value >> 8) & 0xFF])
                pc += 3
        return bytes(out)


# ------------------------------------------------------------------ the program

PPUCTRL, PPUMASK, PPUSTATUS = 0x2000, 0x2001, 0x2002
PPUADDR, PPUDATA, PPUSCROLL = 0x2006, 0x2007, 0x2005
APUFRAME, APUDMC, JOY1 = 0x4017, 0x4010, 0x4016
APUSTATUS = 0x4015
APUPULSE1_CTRL, APUPULSE1_SWEEP = 0x4000, 0x4001
APUPULSE1_LO, APUPULSE1_HI = 0x4002, 0x4003

ZP_BUTTONS, ZP_SCROLL = 0x00, 0x01

# Background palette: universal black, then green / blue / red. Index 1 is the one
# the program repaints when A is held.
GREEN, RED = 0x1A, 0x16
PALETTE = bytes([
    0x0F, GREEN, 0x21, 0x16,   # background palette 0 — used by the whole screen
    0x0F, 0x16, 0x21, 0x1A,
    0x0F, 0x21, 0x1A, 0x16,
    0x0F, 0x30, 0x0F, 0x0F,
] + [0x0F] * 16)               # sprite palettes, unused

# Every attribute byte selects background palette 0 for its 32x32 px cell, so the
# entire screen reacts to the A button rather than just one quadrant.
ATTRIBUTES = bytes([0x00] * 64)


def build_prg():
    a = Assembler(0xC000)

    a.label("reset")
    a.op("sei")
    a.op("cld")
    a.op("lda", 0x40, "imm")
    a.op("sta", APUFRAME)          # silence the APU frame IRQ
    a.op("ldx", 0xFF, "imm")
    a.op("txs")
    a.op("lda", 0x00, "imm")
    a.op("sta", PPUCTRL)           # NMI and rendering off while we set up
    a.op("sta", PPUMASK)
    a.op("sta", APUDMC)
    a.op("sta", ZP_SCROLL, "zp")

    # The PPU is not ready until two vblanks have passed after power-on.
    a.label("vblank1")
    a.op("bit", PPUSTATUS)
    a.op("bpl", "vblank1")
    a.label("vblank2")
    a.op("bit", PPUSTATUS)
    a.op("bpl", "vblank2")

    # Palette → $3F00.
    a.op("lda", 0x3F, "imm")
    a.op("sta", PPUADDR)
    a.op("lda", 0x00, "imm")
    a.op("sta", PPUADDR)
    a.op("ldx", 0x00, "imm")
    a.label("palette_loop")
    a.op("lda", "palette_data", "absx")
    a.op("sta", PPUDATA)
    a.op("inx")
    a.op("cpx", 0x20, "imm")
    a.op("bne", "palette_loop")

    # Nametable 0 → 960 copies of tile 1. Written as 4 x 240 because the loop
    # counter is 8-bit.
    a.op("lda", 0x20, "imm")
    a.op("sta", PPUADDR)
    a.op("lda", 0x00, "imm")
    a.op("sta", PPUADDR)
    a.op("ldy", 0x00, "imm")
    a.label("nt_outer")
    a.op("ldx", 0x00, "imm")
    a.label("nt_inner")
    a.op("lda", 0x01, "imm")
    a.op("sta", PPUDATA)
    a.op("inx")
    a.op("cpx", 0xF0, "imm")       # 240 tiles
    a.op("bne", "nt_inner")
    a.op("iny")
    a.op("cpy", 0x04, "imm")       # x4 = 960
    a.op("bne", "nt_outer")

    # Attribute table → $23C0.
    a.op("lda", 0x23, "imm")
    a.op("sta", PPUADDR)
    a.op("lda", 0xC0, "imm")
    a.op("sta", PPUADDR)
    a.op("ldx", 0x00, "imm")
    a.label("attr_loop")
    a.op("lda", "attribute_data", "absx")
    a.op("sta", PPUDATA)
    a.op("inx")
    a.op("cpx", 0x40, "imm")
    a.op("bne", "attr_loop")

    # A continuous ~440 Hz square wave on pulse channel 1. Without this the ROM is
    # silent, and a silent core cannot distinguish "audio pipeline works" from
    # "audio pipeline drops everything".
    a.op("lda", 0x01, "imm")
    a.op("sta", APUSTATUS)         # enable pulse 1
    a.op("lda", 0xBF, "imm")
    a.op("sta", APUPULSE1_CTRL)    # 50% duty, length halted, constant volume 15
    a.op("lda", 0x08, "imm")
    a.op("sta", APUPULSE1_SWEEP)   # sweep off
    a.op("lda", 0xFD, "imm")
    a.op("sta", APUPULSE1_LO)      # timer 253 -> 1789773/(16*254) = 440.4 Hz
    a.op("lda", 0xF8, "imm")
    a.op("sta", APUPULSE1_HI)      # length load 31, timer high 0

    # Show the background, including the leftmost 8 px.
    a.op("lda", 0x0E, "imm")
    a.op("sta", PPUMASK)

    # ---------------------------------------------------------------- main loop
    a.label("main")
    a.label("wait_vblank")
    a.op("bit", PPUSTATUS)
    a.op("bpl", "wait_vblank")

    # Latch and shift in the standard controller: 8 reads, MSB ends up as A.
    a.op("lda", 0x01, "imm")
    a.op("sta", JOY1)
    a.op("lda", 0x00, "imm")
    a.op("sta", JOY1)
    a.op("ldx", 0x08, "imm")
    a.label("read_pad")
    a.op("lda", JOY1)
    a.op("lsr")                    # bit 0 → carry
    a.op("rol", ZP_BUTTONS, "zp")  # carry → buttons, shifting left
    a.op("dex")
    a.op("bne", "read_pad")

    # A held → repaint background palette entry 1 red, else green.
    a.op("lda", ZP_BUTTONS, "zp")
    a.op("and", 0x80, "imm")       # bit 7 = A
    a.op("beq", "not_pressed")
    a.op("lda", RED, "imm")
    a.op("jmp", "write_colour")
    a.label("not_pressed")
    a.op("lda", GREEN, "imm")
    a.label("write_colour")
    a.op("tay")                    # stash the colour; PPUADDR needs A
    a.op("lda", 0x3F, "imm")
    a.op("sta", PPUADDR)
    a.op("lda", 0x01, "imm")
    a.op("sta", PPUADDR)
    a.op("tya")
    a.op("sta", PPUDATA)

    # Left/Right adjust the horizontal scroll.
    a.op("lda", ZP_BUTTONS, "zp")
    a.op("and", 0x01, "imm")       # bit 0 = Right
    a.op("beq", "no_right")
    a.op("inc", ZP_SCROLL, "zp")
    a.label("no_right")
    a.op("lda", ZP_BUTTONS, "zp")
    a.op("and", 0x02, "imm")       # bit 1 = Left
    a.op("beq", "no_left")
    a.op("dec", ZP_SCROLL, "zp")
    a.label("no_left")

    # Reset the address latch, then write the scroll registers.
    a.op("lda", 0x00, "imm")
    a.op("sta", PPUADDR)
    a.op("sta", PPUADDR)
    a.op("lda", ZP_SCROLL, "zp")
    a.op("sta", PPUSCROLL)
    a.op("lda", 0x00, "imm")
    a.op("sta", PPUSCROLL)
    a.op("jmp", "main")

    # NMI/IRQ are unused but must land somewhere defined.
    a.label("irq")
    a.op("rti")

    a.label("palette_data")
    a.bytes(PALETTE)
    a.label("attribute_data")
    a.bytes(ATTRIBUTES)

    code = a.assemble()
    if len(code) > 16384 - 6:
        raise SystemExit(f"PRG too large: {len(code)} bytes")

    prg = bytearray(b"\x00" * 16384)
    prg[: len(code)] = code
    # Vectors at $FFFA/$FFFC/$FFFE, i.e. the last six bytes of the 16 KB bank.
    nmi, reset, irq = a.labels["irq"], a.labels["reset"], a.labels["irq"]
    prg[0x3FFA:0x4000] = bytes([
        nmi & 0xFF, nmi >> 8,
        reset & 0xFF, reset >> 8,
        irq & 0xFF, irq >> 8,
    ])
    return bytes(prg), a.labels


def build_chr():
    """Tile 0 blank, tile 1 a bordered square. 8 KB pattern table."""
    chr_rom = bytearray(8192)
    # Plane 0 rows: a 1-px black border around a filled 6x6 block.
    tile1 = [0x00, 0x7E, 0x7E, 0x7E, 0x7E, 0x7E, 0x7E, 0x00]
    for i, row in enumerate(tile1):
        chr_rom[16 + i] = row          # tile 1, plane 0
        chr_rom[16 + 8 + i] = 0x00     # tile 1, plane 1 → colour index 1
    return bytes(chr_rom)


def main():
    prg, labels = build_prg()
    chr_rom = build_chr()

    header = bytes([
        0x4E, 0x45, 0x53, 0x1A,  # "NES\x1a"
        0x01,                    # 1 x 16 KB PRG
        0x01,                    # 1 x 8 KB CHR
        0x00,                    # mapper 0, horizontal mirroring
        0x00,                    # NTSC
    ]) + bytes(8)

    rom = header + prg + chr_rom
    out = pathlib.Path(__file__).resolve().parent.parent / "web" / "roms" / "nes-testcart.nes"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(rom)

    print(f"wrote {out.relative_to(out.parents[2])}: {len(rom)} bytes")
    print(f"  reset vector: ${labels['reset']:04X}")
    print(f"  code size:    {labels['palette_data'] - labels['reset']} bytes")
    print("  behaviour:    green grid; A -> red; Left/Right scroll; 440 Hz tone")
    return 0


if __name__ == "__main__":
    sys.exit(main())
