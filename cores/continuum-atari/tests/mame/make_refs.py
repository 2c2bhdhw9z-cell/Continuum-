#!/usr/bin/env python3
"""Makes the picture references for tests/mame_frames.rs: each of cAtari's own test programs run
on MAME 0.289's a7800 (with our stand-in BIOS), every frame snapshotted, turned back into the MARIA
colour values MAME drew (its NTSC palette has 256 different colours, so each maps back to one), and
hashed. tests/references/<case>.txt keeps only those hashes; nothing MAME made is committed.

    MAME=.../arcaderef python3 tests/mame/make_refs.py [case ...]

Run it from a scratch directory: MAME writes cfg/ and the like where it runs.
"""

import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import a78  # noqa: E402

# (case, program, frames, input events: (frame, port tag, mask, value))
CASES = [
    ("zones", "zones", 120, []),
    ("dma", "dma", 60, []),
]


def png_rgb(path):
    d = open(path, "rb").read()
    i, idat, w, h = 8, b"", 0, 0
    while i < len(d):
        n = struct.unpack(">I", d[i:i + 4])[0]
        t, c = d[i + 4:i + 8], d[i + 8:i + 8 + n]
        if t == b"IHDR":
            w, h = struct.unpack(">II", c[:8])
        if t == b"IDAT":
            idat += c
        i += 12 + n
    raw, stride, out, prev, p = zlib.decompress(idat), w * 3, bytearray(), bytearray(w * 3), 0
    for _ in range(h):
        f, line = raw[p], bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        for x in range(stride):
            a = line[x - 3] if x >= 3 else 0
            b = prev[x]
            c = prev[x - 3] if x >= 3 else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out += line
        prev = line
    return w, h, bytes(out)


def fnv(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def run(case, program, frames, events):
    work = os.path.abspath(f"mame-{case}")
    os.makedirs(work, exist_ok=True)
    for f in os.listdir(work):
        if f.endswith(".png"):
            os.remove(os.path.join(work, f))
    lua = [
        "local scr = manager.machine.screens[':screen']",
        "local ports = manager.machine.ioport.ports",
        "local pal = manager.machine.palettes[':palette']",
        "local fh = io.open('palette.txt', 'w')",
        "for i = 0, pal.entries - 1 do fh:write(string.format('%d %08x\\n', i, pal:pen_color(i))) end",
        "fh:close()",
        "local ev = {}",
    ]
    for (fr, tag, mask, value) in events:
        lua.append(f"ev[{fr}] = ev[{fr}] or {{}}")
        lua.append(f"table.insert(ev[{fr}], function() ports['{tag}']:field({mask}):set_value({value}) end)")
    lua.append(
        "sub = emu.add_machine_frame_notifier(function()\n"
        "  local k = scr:frame_number()\n"
        f"  if k < {frames} then scr:snapshot(string.format('f%05d.png', k)) end\n"
        "  if ev[k] then for _, a in ipairs(ev[k]) do a() end end\n"
        f"  if k >= {frames} + 2 then manager.machine:exit() end\n"
        "end)"
    )
    log = a78.run_mame(work, a78.a78(a78.program(program)), "\n".join(lua) + "\n", frames // 50 + 5)
    if "LUA ERROR" in log or "Error" in log and "CHECKSUM" not in log.upper():
        print(log)
    palette = {}
    for line in open(os.path.join(work, "palette.txt")):
        i, c = line.split()
        palette[int(c, 16) & 0xFFFFFF] = int(i)
    assert len(palette) == 256, "MAME's NTSC palette maps back to one colour value each"
    hashes = []
    for k in range(frames):
        w, h, rgb = png_rgb(os.path.join(work, "snap", f"f{k:05}.png"))
        idx = bytes(palette[(rgb[i] << 16) | (rgb[i + 1] << 8) | rgb[i + 2]] for i in range(0, len(rgb), 3))
        hashes.append((w, h, fnv(idx)))
        if os.environ.get("KEEP"):
            open(os.path.join(work, f"f{k:05}.idx"), "wb").write(idx)
    ref = os.path.join(a78.CRATE, "tests", "references", f"{case}.txt")
    os.makedirs(os.path.dirname(ref), exist_ok=True)
    with open(ref, "w") as f:
        f.write(f"# MAME 0.289's a7800 running test-programs/{program}.asm with our stand-in BIOS, made by\n")
        f.write("# tests/mame/make_refs.py: each frame's MARIA colour values (320 x 224), FNV-1a 64.\n")
        f.write(f"program {program}\n")
        for (fr, tag, mask, value) in events:
            f.write(f"event {fr} {tag} {mask} {value}\n")
        for k, (w, h, hh) in enumerate(hashes):
            f.write(f"frame {k} {w}x{h} {hh:016x}\n")
    print(f"{case}: {frames} frames")


def main():
    wanted = sys.argv[1:]
    for case, program, frames, events in CASES:
        if not wanted or case in wanted:
            run(case, program, frames, events)


if __name__ == "__main__":
    main()
