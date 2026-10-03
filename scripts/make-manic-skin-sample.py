#!/usr/bin/env python3
"""Writes docs/samples/continuum-sample-gba.manicskin from docs/samples/manic-skin-gba-info.json.

The package is flat (info.json at the root, like Manic and Delta require) and carries:
  - the background art (sample_gba.png, shared with the Delta sample),
  - two knob pictures for the switch (knob_off.png, knob_on.png),
  - sound.caf, a 30 ms click, 16-bit mono big-endian linear PCM in a CAF container.
Run again after editing the JSON. The Swift check (scripts/check-skins.sh) parses the JSON's items.
"""
from __future__ import annotations

import json
import math
import struct
import sys
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLES = ROOT / "docs/samples"
INFO = SAMPLES / "manic-skin-gba-info.json"
BACKGROUND = SAMPLES / "sample_gba.png"
OUT = SAMPLES / "continuum-sample-gba.manicskin"


def png(width: int, height: int, rgba: tuple[int, int, int, int]) -> bytes:
    raw = b"".join(b"\x00" + bytes(rgba) * width for _ in range(height))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def caf_click(rate: int = 44100, ms: int = 30) -> bytes:
    frames = rate * ms // 1000
    samples = b"".join(
        struct.pack(">h", int(12000 * math.sin(2 * math.pi * 1800 * i / rate) * math.exp(-i / (rate * 0.006))))
        for i in range(frames)
    )
    desc = struct.pack(">dIIIIII", float(rate), int.from_bytes(b"lpcm", "big"), 0, 2, 1, 1, 16)
    out = b"caff" + struct.pack(">HH", 1, 0)
    out += b"desc" + struct.pack(">q", len(desc)) + desc
    out += b"data" + struct.pack(">q", 4 + len(samples)) + struct.pack(">I", 0) + samples
    return out


def main() -> None:
    info = json.loads(INFO.read_text())
    if not info["gameTypeIdentifier"].startswith("public.aoshuang.game."):
        sys.exit("FAIL: the sample must use a Manic identifier")
    files = {
        "info.json": json.dumps(info, indent=2).encode(),
        "sample_gba.png": BACKGROUND.read_bytes(),
        "knob_off.png": png(24, 24, (220, 220, 220, 255)),
        "knob_on.png": png(24, 24, (238, 41, 74, 255)),
        "sound.caf": caf_click(),
    }
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as zf:
        for name, data in files.items():
            zf.writestr(name, data)
    with zipfile.ZipFile(OUT) as zf:
        names = zf.namelist()
        if any("/" in n for n in names):
            sys.exit("FAIL: the package must be flat")
        if zf.read("sound.caf")[:4] != b"caff":
            sys.exit("FAIL: sound.caf is not a CAF file")
    print(f"wrote {OUT.relative_to(ROOT)}: {', '.join(names)}")


if __name__ == "__main__":
    main()
