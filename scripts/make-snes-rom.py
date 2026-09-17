#!/usr/bin/env python3
"""Builds the Super Nintendo test cartridge used to verify the snes9x core.

The fourth cart in the set, and the first that has to talk to a 16-bit CPU. Like the
others it is written to be asserted against by scripts/core-abi-test.mjs:

  * a bordered tile fills the screen, so a test can distinguish "the picture was
    rendered" from "a flat fill was uploaded";
  * the idle colour is white and the pressed colour is red, both chosen so that no two
    carts in this repository idle in the same colour — that is what lets a captured
    frame identify which core produced it;
  * holding right scrolls the playfield one pixel per frame, which only works if
    per-frame register writes reach the PPU during vblank.

The 65816 starts in 6502 emulation mode, so the reset path has to switch to native
mode, widen the registers, and only then talk to the PPU. Register width is tracked by
hand here (`lda_imm8` versus `lda_imm16` are separate methods) because a 16-bit
immediate assembled as 8-bit desynchronises every following instruction, and the
result is a cart that runs for a while and then jumps somewhere arbitrary.

    python3 scripts/make-snes-rom.py     # -> web/roms/snes-testcart.sfc
"""

import pathlib
import sys

ROM_SIZE = 0x8000  # 32 KB LoROM: the only layout a cart this small can have.

# --- PPU / CPU registers (bank $00) ------------------------------------------------
INIDISP = 0x2100  # screen enable + brightness
BGMODE = 0x2105
BG1SC = 0x2107  # BG1 tilemap address and size
BG12NBA = 0x210B  # BG1/BG2 character base
BG1HOFS = 0x210D  # write twice: low 8 bits, then high 2
BG1VOFS = 0x210E
VMAIN = 0x2115  # VRAM address increment mode
VMADDL = 0x2116  # 16-bit store covers VMADDL+VMADDH
VMDATAL = 0x2118  # 16-bit store covers VMDATAL+VMDATAH
CGADD = 0x2121
CGDATA = 0x2122  # 16-bit store writes low then high
TM = 0x212C  # main-screen layer enable
NMITIMEN = 0x4200
HVBJOY = 0x4212  # bit 7 = vblank, bit 0 = auto joypad read busy
JOY1L = 0x4218  # A X L R . . . .
JOY1H = 0x4219  # B Y Select Start Up Down Left Right

# --- colours (SNES CGRAM is 15-bit BGR: %0bbbbbgg gggrrrrr) ------------------------
BLACK = 0x0000
WHITE = 0x7FFF  # idle
RED = 0x001F  # pressed

# --- VRAM layout, in word addresses ------------------------------------------------
VRAM_TILES = 0x0000
VRAM_TILEMAP = 0x0400

# --- APU ports (the 65816 side of the SPC700 mailbox) ------------------------------
APUIO0 = 0x2140
APUIO1 = 0x2141
APUIO2 = 0x2142  # 16-bit store covers APUIO2+APUIO3

# --- direct-page scratch -----------------------------------------------------------
DP_SCROLL = 0x00  # 1 byte: horizontal scroll accumulator
DP_PHASE = 0x10  # 1 byte: how far the APU upload got, readable from a test

# --- SPC700 program layout, in ARAM ------------------------------------------------
# One contiguous upload. The sample directory has to start on a 256-byte boundary
# because the DSP's DIR register is a *page* number, which is what fixes these
# addresses rather than packing everything tightly.
SPC_CODE_ADDR = 0x0200
SPC_DIR_ADDR = 0x0300  # DSP DIR = 0x03
SPC_BRR_ADDR = 0x0304  # immediately after the single directory entry

# 16 samples per BRR block, DSP output at 32 kHz:
#   freq = 32000 * pitch / 0x1000 / 16  ->  pitch = 440 * 65536 / 32000
SPC_PITCH = round(440 * 65536 / 32000)  # 901 = 0x0385


class CPU65816:
    """A small 65816 assembler: only the opcodes this cart uses.

    Two passes, so forward branches read naturally. Every emitter range-checks its
    operand, and the 8/16-bit immediate forms are separate methods rather than being
    inferred — there is no way for this class to know the current M flag, and guessing
    would produce silent corruption rather than an error.
    """

    def __init__(self, origin=0x8000):
        self.origin = origin
        self.code = bytearray()
        self.labels = {}
        self.fixups = []  # (offset, label, kind)

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

    def _imm8(self, value):
        assert 0 <= value <= 0xFF, f"8-bit immediate out of range: {value:#x}"
        self.emit(value)

    def _imm16(self, value):
        assert 0 <= value <= 0xFFFF, f"16-bit value out of range: {value:#x}"
        self.emit(value & 0xFF, (value >> 8) & 0xFF)

    def _rel8(self, label):
        self.fixups.append((len(self.code), label, "rel8"))
        self.emit(0)

    def _abs16(self, label):
        self.fixups.append((len(self.code), label, "abs16"))
        self.emit(0, 0)

    def resolve(self):
        for offset, name, kind in self.fixups:
            assert name in self.labels, f"undefined label {name}"
            target = self.labels[name]
            if kind == "abs16":
                self.code[offset] = target & 0xFF
                self.code[offset + 1] = (target >> 8) & 0xFF
            else:
                delta = target - (self.origin + offset + 1)
                assert -128 <= delta <= 127, f"branch to {name} out of range ({delta})"
                self.code[offset] = delta & 0xFF
        return bytes(self.code)

    # -- mode / flags --
    def sei(self):
        return self.emit(0x78)

    def clc(self):
        return self.emit(0x18)

    def xce(self):
        """Exchange carry with the emulation flag. clc + xce enters native mode."""
        return self.emit(0xFB)

    def rep(self, mask):
        """Reset status bits. #$30 widens A and X/Y to 16 bits."""
        self.emit(0xC2)
        self._imm8(mask)
        return self

    def sep(self, mask):
        """Set status bits. #$20 narrows A to 8 bits."""
        self.emit(0xE2)
        self._imm8(mask)
        return self

    # -- loads / stores --
    def lda_imm8(self, value):
        self.emit(0xA9)
        self._imm8(value)
        return self

    def lda_imm16(self, value):
        self.emit(0xA9)
        self._imm16(value)
        return self

    def ldx_imm16(self, value):
        self.emit(0xA2)
        self._imm16(value)
        return self

    def lda_abs(self, addr):
        self.emit(0xAD)
        self._imm16(addr)
        return self

    def sta_abs(self, addr):
        self.emit(0x8D)
        self._imm16(addr)
        return self

    def stz_abs(self, addr):
        self.emit(0x9C)
        self._imm16(addr)
        return self

    def lda_dp(self, addr):
        self.emit(0xA5)
        self._imm8(addr)
        return self

    def write_cgdata(self, value):
        """CGRAM colour from an 8-bit-A immediate pair.

        `$2122` is *one* register written twice, not a low/high pair like `$2116`.
        A 16-bit store would put the high byte in `$2123` — the BG1/BG2 window mask —
        which corrupts the window settings and writes half a colour.
        """
        self.lda_imm8(value & 0xFF)
        self.sta_abs(CGDATA)
        self.lda_imm8((value >> 8) & 0xFF)
        self.sta_abs(CGDATA)
        return self

    def sta_dp(self, addr):
        self.emit(0x85)
        self._imm8(addr)
        return self

    def inc_dp(self, addr):
        self.emit(0xE6)
        self._imm8(addr)
        return self

    def lda_absx_label(self, label):
        """lda label,x — used to walk the tile table. Resolved in the second pass."""
        self.emit(0xBD)
        self._abs16(label)
        return self

    def cpx_imm16(self, value):
        self.emit(0xE0)
        self._imm16(value)
        return self

    def cmp_imm8(self, value):
        self.emit(0xC9)
        self._imm8(value)
        return self

    def cmp_abs(self, addr):
        self.emit(0xCD)
        self._imm16(addr)
        return self

    def adc_imm8(self, value):
        self.emit(0x69)
        self._imm8(value)
        return self

    def ldy_imm16(self, value):
        self.emit(0xA0)
        self._imm16(value)
        return self

    def tya(self):
        return self.emit(0x98)

    def iny(self):
        return self.emit(0xC8)

    # -- transfers --
    def txs(self):
        return self.emit(0x9B)

    def tcd(self):
        """Transfer C (16-bit A) to the direct page register."""
        return self.emit(0x5B)

    # -- arithmetic / logic --
    def and_imm8(self, value):
        self.emit(0x29)
        self._imm8(value)
        return self

    def dex(self):
        return self.emit(0xCA)

    def inx(self):
        return self.emit(0xE8)

    # -- control flow --
    def bne(self, label):
        self.emit(0xD0)
        self._rel8(label)
        return self

    def beq(self, label):
        self.emit(0xF0)
        self._rel8(label)
        return self

    def bra(self, label):
        self.emit(0x80)
        self._rel8(label)
        return self

    def jmp(self, label):
        self.emit(0x4C)
        self._abs16(label)
        return self

    def rti(self):
        return self.emit(0x40)


def spc_payload():
    """The SPC700 program, sample directory and BRR sample, as one ARAM image.

    The SNES is the only system in this set whose audio needs a *second* program: the
    DSP registers live behind the SPC700's own address space, so the 65816 cannot
    reach them. It uploads this blob through the APU boot ROM and jumps to it.

    The program itself is only DSP register writes. Every one is `mov $F2,#reg`
    followed by `mov $F3,#val` — SPC700 opcode 0x8F, which stores an immediate to a
    direct-page address with the operands in that order (immediate first).
    """
    code = bytearray()

    def dsp(reg, val):
        code.extend((0x8F, reg, 0xF2))  # mov $F2, #reg   (DSP address latch)
        code.extend((0x8F, val, 0xF3))  # mov $F3, #val   (DSP data)

    # Mute while the voice is configured. On boot FLG is 0xE0 (reset + mute + echo
    # disabled), so clearing the reset bit at the end is what starts the DSP.
    dsp(0x6C, 0x20)  # FLG: mute on, reset off
    dsp(0x5D, SPC_DIR_ADDR >> 8)  # DIR: sample directory page
    dsp(0x0C, 0x7F)  # MVOL left
    dsp(0x1C, 0x7F)  # MVOL right
    dsp(0x2C, 0x00)  # EVOL left  (echo silent)
    dsp(0x3C, 0x00)  # EVOL right
    dsp(0x4D, 0x00)  # EON: no voice routed to echo
    dsp(0x5C, 0x00)  # KOFF: nothing keyed off
    dsp(0x00, 0x7F)  # voice 0 VOL left
    dsp(0x01, 0x7F)  # voice 0 VOL right
    dsp(0x02, SPC_PITCH & 0xFF)  # voice 0 PITCH low
    dsp(0x03, (SPC_PITCH >> 8) & 0xFF)  # voice 0 PITCH high
    dsp(0x04, 0x00)  # voice 0 SRCN: directory entry 0
    dsp(0x05, 0x00)  # voice 0 ADSR1: 0 selects direct GAIN
    dsp(0x07, 0x7F)  # voice 0 GAIN: maximum, no envelope
    dsp(0x6C, 0x00)  # FLG: unmute
    dsp(0x4C, 0x01)  # KON: key on voice 0
    code.extend((0x2F, 0xFE))  # bra to self: the DSP plays on without the CPU

    assert SPC_CODE_ADDR + len(code) <= SPC_DIR_ADDR, "SPC code runs into the directory"

    # A 16-sample square wave in one BRR block. Range 11 shifts each nibble left by
    # 10, so +7/-8 becomes a loud tone without needing an envelope. Setting both the
    # loop and end bits makes the block repeat forever from its own start.
    brr = bytes([0xB3]) + bytes([0x77] * 4 + [0x88] * 4)

    image = bytearray(b"\x00" * (SPC_BRR_ADDR + len(brr) - SPC_CODE_ADDR))
    image[0 : len(code)] = code
    # Directory entry 0: start address, then loop address.
    entry = SPC_DIR_ADDR - SPC_CODE_ADDR
    image[entry : entry + 4] = bytes(
        [
            SPC_BRR_ADDR & 0xFF,
            SPC_BRR_ADDR >> 8,
            SPC_BRR_ADDR & 0xFF,
            SPC_BRR_ADDR >> 8,
        ]
    )
    image[SPC_BRR_ADDR - SPC_CODE_ADDR :] = brr
    return bytes(image)


def emit_apu_upload(a, blob_len):
    """The 65816 side of the APU boot-ROM handshake.

    The protocol is a lockstep mailbox: every byte written to `$2141` is acknowledged
    by the boot ROM echoing the counter back through `$2140`, and each step spins until
    it sees that echo. A single wrong constant therefore hangs rather than misbehaves,
    which is why each stage records how far it got in `DP_PHASE` — a test can read that
    byte out of work RAM and say exactly which handshake stalled.
    """
    a.lda_imm8(0x01)
    a.sta_dp(DP_PHASE)

    # The boot ROM signals readiness with 0xAA / 0xBB.
    a.label("apu_ready")
    a.lda_abs(APUIO0)
    a.cmp_imm8(0xAA)
    a.bne("apu_ready")
    a.lda_abs(APUIO1)
    a.cmp_imm8(0xBB)
    a.bne("apu_ready")
    a.lda_imm8(0x02)
    a.sta_dp(DP_PHASE)

    # Destination address, then a non-zero byte in $2141 meaning "data follows", then
    # any non-zero kick value in $2140.
    a.rep(0x20)
    a.lda_imm16(SPC_CODE_ADDR)
    a.sta_abs(APUIO2)  # $2142/$2143 really are a low/high pair
    a.sep(0x20)
    a.lda_imm8(0x01)
    a.sta_abs(APUIO1)
    a.lda_imm8(0xCC)
    a.sta_abs(APUIO0)
    a.label("apu_kick")
    a.lda_abs(APUIO0)
    a.cmp_imm8(0xCC)
    a.bne("apu_kick")
    a.lda_imm8(0x03)
    a.sta_dp(DP_PHASE)

    # Byte loop. X walks the blob, Y is the counter the boot ROM echoes back; it is
    # allowed to wrap, which it does naturally when the low byte lands in an 8-bit A.
    a.rep(0x10)  # 16-bit X/Y, A stays 8-bit
    a.ldx_imm16(0)
    a.ldy_imm16(0)
    a.label("apu_xfer")
    a.lda_absx_label("spc_blob")
    a.sta_abs(APUIO1)
    a.tya()
    a.sta_abs(APUIO0)
    a.label("apu_ack")
    a.cmp_abs(APUIO0)
    a.bne("apu_ack")
    a.inx()
    a.iny()
    a.cpx_imm16(blob_len)
    a.bne("apu_xfer")
    a.lda_imm8(0x04)
    a.sta_dp(DP_PHASE)

    # End of transfer: zero in $2141 means "no more blocks, jump to the address in
    # $2142". The counter advances by two rather than one to distinguish this from an
    # ordinary byte. Nothing waits for an echo afterwards — the boot ROM has already
    # jumped, so the mailbox will never change again.
    a.lda_imm8(0x00)
    a.sta_abs(APUIO1)
    a.rep(0x20)
    a.lda_imm16(SPC_CODE_ADDR)
    a.sta_abs(APUIO2)
    a.sep(0x20)
    a.tya()
    a.clc()
    a.adc_imm8(0x02)
    a.sta_abs(APUIO0)
    a.lda_imm8(0x05)
    a.sta_dp(DP_PHASE)
    return a


def tile_data():
    """Two 2bpp tiles for BG mode 0: 8 rows of [bitplane0, bitplane1], 16 bytes each.

    Tile 0 is blank. Tile 1 is a 6x6 block of colour 1 inside a colour-0 border, the
    same shape the other carts use, so a screen tiled with it is 28 dark and 36 bright
    pixels per tile rather than a uniform field a "did it draw" check could not tell
    from an upload of a single colour.
    """
    rows = bytearray()
    for y in range(8):
        # Bit 7 is the leftmost pixel, so 0x7E lights columns 1..6.
        plane0 = 0x00 if y in (0, 7) else 0x7E
        rows += bytes([plane0, 0x00])  # bitplane 1 stays clear: colour index 1
    return bytes(16) + bytes(rows)  # tile 0 blank, then tile 1


SPC_BLOB = None  # built in main(), before the 65816 program that embeds it


def build_program():
    a = CPU65816(origin=0x8000)

    # ---- reset: emulation mode, 8-bit, interrupts on ----
    a.sei()
    a.clc()
    a.xce()  # native mode
    a.rep(0x38)  # 16-bit A/X/Y, decimal off
    a.ldx_imm16(0x01FF)
    a.txs()  # stack at $01FF
    a.lda_imm16(0x0000)
    a.tcd()  # direct page at $0000
    a.sep(0x20)  # 8-bit A, X/Y stay 16-bit

    # ---- PPU: forced blank while VRAM and CGRAM are written ----
    a.lda_imm8(0x8F)
    a.sta_abs(INIDISP)
    a.stz_abs(BGMODE)  # mode 0: every BG is 2bpp, which is all this cart needs
    a.lda_imm8(0x04)  # tilemap at word $0400, 32x32
    a.sta_abs(BG1SC)
    a.stz_abs(BG12NBA)  # BG1 characters at word $0000
    a.stz_abs(BG1VOFS)
    a.stz_abs(BG1VOFS)  # both writes: this register is write-twice
    a.lda_imm8(0x80)
    a.sta_abs(VMAIN)  # increment one word after the high-byte write

    # ---- palette: CGRAM[0] = black (the tile border), CGRAM[1] = white ----
    # CGADD advances by one entry after every second byte written, so consecutive
    # pairs fill successive entries.
    a.stz_abs(CGADD)
    a.write_cgdata(BLACK)
    a.write_cgdata(WHITE)

    # ---- tiles ----
    # Uploaded as words, not bytes. VMAIN is set to increment after the *high* byte
    # write, so a run of 8-bit stores to VMDATAL would rewrite word zero 32 times and
    # leave every tile blank.
    a.rep(0x30)  # 16-bit A and X
    a.lda_imm16(VRAM_TILES)
    a.sta_abs(VMADDL)
    a.ldx_imm16(0)
    a.label("tile_loop")
    a.lda_absx_label("tiles")
    a.sta_abs(VMDATAL)  # 16-bit store covers VMDATAL and VMDATAH
    a.inx()
    a.inx()
    a.cpx_imm16(32)  # 32 bytes = 16 words = two 2bpp tiles
    a.bne("tile_loop")
    a.sep(0x20)

    # ---- tilemap: 32x32 entries of tile 1, palette 0 ----
    a.rep(0x20)
    a.lda_imm16(VRAM_TILEMAP)
    a.sta_abs(VMADDL)
    a.ldx_imm16(0x0400)  # 1024 entries
    a.lda_imm16(0x0001)  # tile 1
    a.label("map_loop")
    a.sta_abs(VMDATAL)  # 16-bit store fills VMDATAL and VMDATAH
    a.dex()
    a.bne("map_loop")
    a.sep(0x20)

    # ---- scroll accumulator, auto joypad read, screen on ----
    a.lda_imm8(0x00)
    a.sta_dp(DP_SCROLL)
    a.lda_imm8(0x01)
    a.sta_abs(NMITIMEN)  # bit 0 = auto joypad read; NMI stays disabled
    a.lda_imm8(0x01)
    a.sta_abs(TM)  # BG1 on the main screen
    a.lda_imm8(0x0F)
    a.sta_abs(INIDISP)  # screen on, full brightness

    # ---- audio ----
    # Deliberately after the screen is enabled: if the handshake ever stalls, the
    # picture is already on, so "the APU upload hung" looks different from "the cart
    # is dead", and DP_PHASE says which stage stopped.
    emit_apu_upload(a, len(SPC_BLOB))

    # ---- main loop, one iteration per frame ----
    a.label("frame")
    # Wait for the vblank flag to clear first: entering the loop already inside vblank
    # would otherwise run two iterations for one frame.
    a.label("wait_active")
    a.lda_abs(HVBJOY)
    a.and_imm8(0x80)
    a.bne("wait_active")
    a.label("wait_vblank")
    a.lda_abs(HVBJOY)
    a.and_imm8(0x80)
    a.beq("wait_vblank")
    # The controller latch is only stable once the automatic read has finished.
    a.label("wait_joy")
    a.lda_abs(HVBJOY)
    a.and_imm8(0x01)
    a.bne("wait_joy")

    # A ($4218 bit 7) or B ($4219 bit 7) turns the playfield red, so the cart responds
    # whichever way a frontend maps its face buttons.
    a.lda_abs(JOY1L)
    a.and_imm8(0x80)
    a.bne("pressed")
    a.lda_abs(JOY1H)
    a.and_imm8(0x80)
    a.bne("pressed")
    a.lda_imm8(0x01)
    a.sta_abs(CGADD)  # CGRAM index 1
    a.write_cgdata(WHITE)
    a.bra("after_colour")
    a.label("pressed")
    a.lda_imm8(0x01)
    a.sta_abs(CGADD)
    a.write_cgdata(RED)
    a.label("after_colour")

    # Right ($4219 bit 0) scrolls one pixel per frame.
    a.lda_abs(JOY1H)
    a.and_imm8(0x01)
    a.beq("no_scroll")
    a.inc_dp(DP_SCROLL)
    a.label("no_scroll")
    a.lda_dp(DP_SCROLL)
    a.sta_abs(BG1HOFS)  # low 8 bits
    a.stz_abs(BG1HOFS)  # high 2 bits
    a.jmp("frame")

    a.label("tiles")
    a.emit(*tile_data())
    a.label("spc_blob")
    a.emit(*SPC_BLOB)

    return a


def main():
    global SPC_BLOB
    SPC_BLOB = spc_payload()

    program = build_program()
    code = bytearray(program.resolve())
    tiles_addr = program.labels["tiles"]

    rom = bytearray(b"\x00" * ROM_SIZE)
    rom[0 : len(code)] = code
    end = len(code)
    assert end <= 0x7FC0, f"program overruns the header at 0x7FC0 (ends {end:#06x})"

    # ---- LoROM header at file offset $7FC0 (CPU $00:FFC0) ----
    title = b"CONTINUUM TEST CART  "  # exactly 21 bytes
    assert len(title) == 21
    rom[0x7FC0:0x7FD5] = title
    rom[0x7FD5] = 0x20  # LoROM, slow ROM
    rom[0x7FD6] = 0x00  # ROM only, no coprocessor
    rom[0x7FD7] = 0x05  # 1 << 5 KB = 32 KB
    rom[0x7FD8] = 0x00  # no SRAM
    rom[0x7FD9] = 0x01  # NTSC / North America
    rom[0x7FDA] = 0x33  # licensee: use the extended header convention
    rom[0x7FDB] = 0x00  # version

    # ---- vectors ----
    # Native mode occupies $FFE4-$FFEF, emulation mode $FFF4-$FFFF. Only reset is
    # reachable in this cart, but pointing the interrupt vectors at an `rti` beats
    # letting a stray one execute whatever happens to be at address zero.
    stub = CPU65816(origin=0x8000)
    stub.rti()
    rti_at = end
    rom[rti_at] = stub.resolve()[0]
    rti_vec = 0x8000 + rti_at

    def vec(offset, addr):
        rom[offset] = addr & 0xFF
        rom[offset + 1] = (addr >> 8) & 0xFF

    for off in (0x7FE4, 0x7FE6, 0x7FE8, 0x7FEA, 0x7FEC, 0x7FEE):
        vec(off, rti_vec)  # native COP/BRK/ABORT/NMI/-/IRQ
    for off in (0x7FF4, 0x7FF8, 0x7FFA, 0x7FFE):
        vec(off, rti_vec)  # emulation COP/ABORT/NMI/IRQ
    vec(0x7FFC, 0x8000)  # emulation RESET: where the CPU actually starts

    # ---- checksum ----
    # The complement and the checksum always sum to 0x01FE byte-wise, whatever their
    # value, so the fields are zeroed, the rest summed, and the constant added back.
    rom[0x7FDC:0x7FE0] = b"\x00\x00\x00\x00"
    checksum = (sum(rom) + 0x01FE) & 0xFFFF
    complement = checksum ^ 0xFFFF
    vec(0x7FDC, complement)
    vec(0x7FDE, checksum)

    out = pathlib.Path(__file__).resolve().parent.parent / "web" / "roms" / "snes-testcart.sfc"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(bytes(rom))

    print(f"wrote {out.relative_to(out.parent.parent.parent)} ({len(rom)} bytes)")
    print(f"  program {end} bytes at $8000-${0x8000 + end - 1:04X}, tiles at ${tiles_addr:04X}")
    print(f"  idle white ${WHITE:04X}, pressed red ${RED:04X}, checksum ${checksum:04X}")
    print(
        f"  SPC700 payload {len(SPC_BLOB)} bytes -> ARAM ${SPC_CODE_ADDR:04X}, "
        f"pitch ${SPC_PITCH:04X} (~440 Hz)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
