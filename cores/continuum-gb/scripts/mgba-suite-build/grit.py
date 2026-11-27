#!/usr/bin/env python3
"""Minimal replacement for devkitPro's `grit`, just enough to build mgba-emu/suite
with a stock arm-none-eabi-gcc (no devkitARM, no libgba, no grit on the VM).

Reads the suite's 4bpp BMPs under test-roms/mgba-suite/gfx/raw/ and emits, for
each .grit file, a C header + an ARM assembly data file with the symbols the
suite's C code uses (grit's naming: <base>Tiles, <base>TilesLen, <base>Pal,
<base>PalLen, <base>Map, <base>MapLen).

Formats (matching what grit produces for these inputs):
- Tiles: 4bpp, 8x8 pixels, 32 bytes each, left-to-right, top-to-bottom, pixels
  packed low nibble first. No tile deduplication (grit's default here), so the
  map is just sequential tile indices.
- Palette: BMP palette entries (B,G,R,0) converted to GBA BGR555, 16 entries
  for "-pe 16".
- Map: u16 tile indices, 32 columns x rows for the image, row-major.
- LZ77 ("-gzl"/"-mzl"): the GBA BIOS format (0x10 header + 24-bit size, flag
  bytes MSB-first, back-references of length (hi nibble)+3, displacement
  (lo nibble + next byte)+1). The suite decompresses with the BIOS SWI 0x11.

Usage: grit.py <out-dir>
"""
import os
import struct
import sys


def read_bmp(path):
    d = open(path, "rb").read()
    assert d[:2] == b"BM"
    off = struct.unpack_from("<I", d, 10)[0]
    w = struct.unpack_from("<i", d, 18)[0]
    h = struct.unpack_from("<i", d, 22)[0]
    bpp = struct.unpack_from("<H", d, 28)[0]
    assert bpp == 4, (path, bpp)
    ncolors = struct.unpack_from("<I", d, 46)[0] or 16
    pal = []
    for i in range(ncolors):
        b, g, r, _ = d[14 + 40 + i * 4: 14 + 40 + i * 4 + 4]
        pal.append((r >> 3) | ((g >> 3) << 5) | ((b >> 3) << 10))
    # Rows are bottom-up; each row is padded to a multiple of 4 bytes.
    stride = ((w + 7) // 8) * 4  # bytes per row for 4bpp: w/2 rounded up to 4
    rows = []
    for y in range(h):
        src = off + (h - 1 - y) * stride
        row = []
        for x in range(w):
            byte = d[src + x // 2]
            row.append((byte >> 4) & 0xF if x % 2 == 0 else byte & 0xF)
        rows.append(row)
    return w, h, pal, rows


def make_tiles(w, h, rows):
    assert w % 8 == 0 and h % 8 == 0
    out = bytearray()
    for ty in range(h // 8):
        for tx in range(w // 8):
            for y in range(8):
                for x in range(0, 8, 2):
                    lo = rows[ty * 8 + y][tx * 8 + x]
                    hi = rows[ty * 8 + y][tx * 8 + x + 1]
                    out.append(lo | (hi << 4))
    return bytes(out)


def make_map(w, h):
    n = (w // 8) * (h // 8)
    return b"".join(struct.pack("<H", i) for i in range(n))


def lz77(data):
    """Greedy GBA BIOS LZ77 compressor."""
    out = bytearray()
    out.append(0x10)
    out.extend(struct.pack("<I", len(data))[:3])
    i = 0
    n = len(data)
    while i < n:
        flag_pos = len(out)
        out.append(0)
        flag = 0
        for bit in range(8):
            if i >= n:
                break
            best_len, best_disp = 0, 0
            start = max(0, i - 4096)
            # Longest match at or after start, length 3..18.
            for j in range(start, i):
                l = 0
                while l < 18 and i + l < n and data[j + l] == data[i + l]:
                    l += 1
                if l >= 3 and l > best_len:
                    best_len, best_disp = l, i - j
                    if l == 18:
                        break
            if best_len >= 3:
                flag |= 0x80 >> bit
                b1 = ((best_len - 3) << 4) | ((best_disp - 1) >> 8)
                b2 = (best_disp - 1) & 0xFF
                out.append(b1)
                out.append(b2)
                i += best_len
            else:
                out.append(data[i])
                i += 1
        out[flag_pos] = flag
    return bytes(out)


def emit_words(f, name, data):
    """Emit `name` as an array of .word values, padded to a multiple of 4."""
    padded = data + b"\x00" * ((-len(data)) % 4)
    f.write(f"\t.global {name}\n\t.align 2\n{name}:\n")
    for i in range(0, len(padded), 16):
        chunk = padded[i:i + 16]
        words = ", ".join("0x%08X" % struct.unpack_from("<I", chunk, j)[0]
                          for j in range(0, len(chunk), 4))
        f.write(f"\t.word {words}\n")


def emit_hwords(f, name, data):
    f.write(f"\t.global {name}\n\t.align 2\n{name}:\n")
    for i in range(0, len(data), 16):
        chunk = data[i:i + 16]
        vals = ", ".join("0x%04X" % struct.unpack_from("<H", chunk, j)[0]
                         for j in range(0, len(chunk), 2))
        f.write(f"\t.hword {vals}\n")


def gen(outdir, base, bmp, palette=True, compress_tiles=False, gen_map=False,
        compress_map=False):
    w, h, pal, rows = read_bmp(bmp)
    tiles = make_tiles(w, h, rows)
    tile_data = lz77(tiles) if compress_tiles else tiles
    pal16 = (pal + [0] * 16)[:16]
    pal_data = b"".join(struct.pack("<H", c) for c in pal16)
    map_data = None
    if gen_map:
        raw_map = make_map(w, h)
        map_data = lz77(raw_map) if compress_map else raw_map

    hdr = [f"// Generated by scripts/mgba-suite-build/grit.py from {os.path.basename(bmp)}.",
           f"// grit options: {'-pe 16 ' if palette else '-p! '}"
           f"{'-gzl ' if compress_tiles else ''}"
           f"{'-m -mzl' if gen_map and compress_map else ''}".rstrip(),
           "#ifndef %s_H" % base.upper(),
           "#define %s_H" % base.upper(),
           "",
           "#define %sTilesLen %d" % (base, len(tile_data)),
           "extern const unsigned int %sTiles[%d];"
           % (base, (len(tile_data) + 3) // 4)]
    if palette:
        hdr += ["#define %sPalLen %d" % (base, len(pal_data)),
                "extern const unsigned short %sPal[%d];" % (base, len(pal_data) // 2)]
    if map_data is not None:
        hdr += ["#define %sMapLen %d" % (base, len(map_data)),
                "extern const unsigned int %sMap[%d];"
                % (base, (len(map_data) + 3) // 4)]
    hdr += ["", "#endif"]
    with open(os.path.join(outdir, base + ".h"), "w") as f:
        f.write("\n".join(hdr) + "\n")

    with open(os.path.join(outdir, base + ".s"), "w") as f:
        f.write("\t.section .rodata\n")
        emit_words(f, base + "Tiles", tile_data)
        if palette:
            emit_hwords(f, base + "Pal", pal_data)
        if map_data is not None:
            emit_words(f, base + "Map", map_data)


def main():
    outdir = sys.argv[1]
    os.makedirs(outdir, exist_ok=True)
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        "..", "..", "test-roms", "mgba-suite", "gfx", "raw")
    gen(outdir, "font", os.path.join(root, "font.bmp"),
        palette=True, compress_tiles=False)
    gen(outdir, "actual", os.path.join(root, "actual.bmp"),
        palette=False, compress_tiles=True)
    gen(outdir, "expected", os.path.join(root, "expected.bmp"),
        palette=False, compress_tiles=True)
    gen(outdir, "degenerateObjTransform",
        os.path.join(root, "degenerateObjTransform.bmp"),
        palette=True, compress_tiles=True, gen_map=True, compress_map=True)
    print("grit.py: wrote font/actual/expected/degenerateObjTransform .h/.s to", outdir)


if __name__ == "__main__":
    main()
