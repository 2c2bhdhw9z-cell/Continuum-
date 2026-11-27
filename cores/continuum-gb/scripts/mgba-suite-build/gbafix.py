#!/usr/bin/env python3
"""Minimal replacement for devkitPro's `gbafix`: fills in the GBA ROM header
of a linked .gba — the Nintendo logo (copied from a known-good ROM on the VM),
the 0x96 fixed byte, and the header complement check at 0xBD.

Usage: gbafix.py <reference.gba> <in-out.gba>
"""
import sys

LOGO_OFF, LOGO_LEN = 0x04, 0x9C


def main():
    ref, target = sys.argv[1], sys.argv[2]
    with open(ref, "rb") as f:
        logo = f.read()[LOGO_OFF:LOGO_OFF + LOGO_LEN]
    assert len(logo) == LOGO_LEN
    with open(target, "r+b") as f:
        data = bytearray(f.read())
        data[LOGO_OFF:LOGO_OFF + LOGO_LEN] = logo
        data[0xB2] = 0x96
        # Complement check: -(0x19 + sum of bytes 0xA0..0xBC) mod 256.
        data[0xBD] = (-(0x19 + sum(data[0xA0:0xBD]))) & 0xFF
        f.seek(0)
        f.write(data)
    print("gbafix.py: fixed header of", target)


if __name__ == "__main__":
    main()
