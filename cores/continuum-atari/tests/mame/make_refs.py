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

def press_each():
    """Every stick direction, button and switch, one at a time, three frames each, then the two
    difficulty switches to B and back."""
    ev, f = [], 6
    for tag, masks in [(":JOYSTICKS", [0x10, 0x20, 0x40, 0x80, 0x01, 0x02, 0x04, 0x08]),
                       (":BUTTONS", [0x08, 0x02, 0x04, 0x01]), (":CONSOLE", [0x01, 0x02, 0x08])]:
        for m in masks:
            ev.append((f, tag, m, 1))
            ev.append((f + 3, tag, m, 0))
            f += 5
    # Two at once: up and fire, both buttons.
    ev += [(f, ":JOYSTICKS", 0x10, 1), (f, ":BUTTONS", 0x08, 1), (f + 3, ":JOYSTICKS", 0x10, 0), (f + 3, ":BUTTONS", 0x08, 0)]
    f += 5
    ev += [(f, ":BUTTONS", 0x08, 1), (f, ":BUTTONS", 0x02, 1), (f + 3, ":BUTTONS", 0x08, 0), (f + 3, ":BUTTONS", 0x02, 0)]
    f += 5
    # The difficulty switches, A to B: "pressing" a DIP field in MAME's Lua moves it (one frame
    # later than a button, and letting go doesn't move it back).
    ev += [(f, ":CONSOLE", 0x40, 1), (f + 3, ":CONSOLE", 0x80, 1)]
    return ev


# (case, program, frames, input events: (frame, port tag, mask, value); a "dip:" tag sets a DIP)
CASES = [
    ("zones", "zones", 120, []),
    ("dma", "dma", 60, []),
    ("modes", "modes", 70, []),
    ("inputs", "inputs", 110, press_each()),
    ("banks_sg", "banks_sg", 20, []),
    ("banks_sg9", "banks_sg9", 20, []),
    ("banks_sgram", "banks_sgram", 20, []),
    ("banks_abs", "banks_abs", 20, []),
    ("banks_act", "banks_act", 20, []),
    ("hsc", "hsc", 12, []),
    ("zones_pal", "zones", 60, []),
    ("modes_pal", "modes", 70, []),
]

# The cases played on MAME's PAL console (a7800p). Its palette shows hue 15 as hue 1, so the
# colour values are compared with hue 15 counted as hue 1.
PAL = {"zones_pal", "modes_pal"}

# The cases played with the High Score Cartridge between the console and the cartridge.
HSC = {"hsc"}


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
        if tag.startswith("dip:"):
            lua.append(f"table.insert(ev[{fr}], function() ports['{tag[4:]}']:field({mask}).user_value = {value} end)")
        else:
            lua.append(f"table.insert(ev[{fr}], function() ports['{tag}']:field({mask}):set_value({value}) end)")
    lua.append(
        "sub = emu.add_machine_frame_notifier(function()\n"
        "  local k = scr:frame_number()\n"
        f"  if k < {frames} then scr:snapshot(string.format('f%05d.png', k)) end\n"
        "  if ev[k] then for _, a in ipairs(ev[k]) do a() end end\n"
        f"  if k >= {frames} + 2 then manager.machine:exit() end\n"
        "end)"
    )
    pal = case in PAL
    log = a78.run_mame(work, a78.cartridge(program), "\n".join(lua) + "\n", frames // 50 + 5, hsc=case in HSC, pal=pal)
    if "LUA ERROR" in log or "Error" in log and "CHECKSUM" not in log.upper():
        print(log)
    palette = {}
    for line in open(os.path.join(work, "palette.txt")):
        i, c = line.split()
        # The first colour value with each colour (on PAL, hue 1 for hue 15).
        palette.setdefault(int(c, 16) & 0xFFFFFF, int(i))
    if pal:
        assert len(palette) == 240 and all(palette[k] < 0xF0 for k in palette), "PAL: hue 15 shows as hue 1"
    else:
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
        f.write(f"# tests/mame/make_refs.py: each frame's MARIA colour values ({'320 x 260, hue 15 as hue 1' if pal else '320 x 224'}), FNV-1a 64.\n")
        f.write(f"program {program}\n")
        if case in HSC:
            f.write("hsc\n")
        if pal:
            f.write("pal\n")
        for (fr, tag, mask, value) in events:
            f.write(f"event {fr} {tag} {mask} {value}\n")
        for k, (w, h, hh) in enumerate(hashes):
            f.write(f"frame {k} {w}x{h} {hh:016x}\n")
    print(f"{case}: {frames} frames")


# (case, program, frames): the TIA's sound, recorded at the chip's own rate (MAME's 31,399 Hz),
# where MAME's recording is the TIA's level times 1024 with nothing in between.
SOUND = [
    ("sound", "sound", 400),
]
BLOCK = 4096
# MAME's recording starts this many samples before the machine's first sound step (tests/tia_mame.rs
# starts our TIA's sound clock where MAME's second sample is).
MAME_LEAD = 1


def sound(case, program, frames):
    work = os.path.abspath(f"mame-{case}")
    os.makedirs(work, exist_ok=True)
    lua = ("sub = emu.add_machine_frame_notifier(function() "
           f"if manager.machine.screens[':screen']:frame_number() >= {frames} then manager.machine:exit() end end)\n")
    a78.run_mame(work, a78.cartridge(program), lua, frames // 50 + 5,
                 extra=("-samplerate", "31399", "-wavwrite", "sound.wav"))
    d = open(os.path.join(work, "sound.wav"), "rb").read()
    j = d.find(b"data")
    n = struct.unpack("<I", d[j + 4:j + 8])[0]
    raw = struct.unpack(f"<{n // 2}h", d[j + 8:j + 8 + n])
    assert all(v % 1024 == 0 for v in raw), "MAME's recording is the TIA's levels, untouched"
    levels = bytes(v // 1024 for v in raw)[MAME_LEAD:]
    # Only whole blocks, and not the last one (the recording stops part-way through a frame).
    count = (len(levels) // BLOCK - 1) * BLOCK
    ref = os.path.join(a78.CRATE, "tests", "references", f"{case}.txt")
    with open(ref, "w") as f:
        f.write(f"# MAME 0.289's a7800 playing test-programs/{program}.asm's TIA sound at 31,399 Hz, made by\n")
        f.write(f"# tests/mame/make_refs.py: the levels (0-30) after its {MAME_LEAD}-sample lead, FNV-1a 64 of each {BLOCK}.\n")
        f.write(f"program {program}\nframes {frames}\nsamples {count}\n")
        for k in range(0, count, BLOCK):
            f.write(f"block {k // BLOCK} {fnv(levels[k:k + BLOCK]):016x}\n")
    if os.environ.get("KEEP"):
        open(os.path.join(work, "levels.bin"), "wb").write(levels)
    print(f"{case}: {count} samples")


# (case, program, frames): POKEY's sound. MAME can't record at POKEY's 1,789,772 Hz, so it records
# at 894,887 Hz through its resampler; this only makes the recording (mame-<case>/<case>.wav).
# tests/pokey_mame.rs checks our POKEY against it and writes tests/references/<case>.txt:
#     MAME_POKEY=.../mame-pokey/pokey.wav WRITE_REF=1 cargo test --release --test pokey_mame
POKEY_SOUND = [
    ("pokey", "pokey", 210),
]


# (case, program, frames): the XM expansion module's YM2151, recorded at the chip's own rate
# (3,579,544 / 64 = 55,930 Hz, which MAME can record). MAME puts the XM's sound on a speaker of
# its own, so the second recorded channel is the chip's two sides added.
# tests/ym2151_mame.rs checks our chip against it, sample for sample.
YM_SOUND = [
    ("xm", "xm", 240),
]


def ym_sound(case, program, frames):
    work = os.path.abspath(f"mame-{case}")
    os.makedirs(work, exist_ok=True)
    lua = ("sub = emu.add_machine_frame_notifier(function() "
           f"if manager.machine.screens[':screen']:frame_number() >= {frames} + 2 then manager.machine:exit() end end)\n")
    a78.run_mame(work, a78.cartridge(program), lua, frames // 50 + 5, xm=True,
                 extra=("-samplerate", "55930", "-wavwrite", f"{case}.wav"))
    print(f"{case}: {os.path.join(work, case + '.wav')}")


def pokey_sound(case, program, frames):
    work = os.path.abspath(f"mame-{case}")
    os.makedirs(work, exist_ok=True)
    lua = ("sub = emu.add_machine_frame_notifier(function() "
           f"if manager.machine.screens[':screen']:frame_number() >= {frames} + 2 then manager.machine:exit() end end)\n")
    a78.run_mame(work, a78.cartridge(program), lua, frames // 50 + 5,
                 extra=("-samplerate", "894887", "-wavwrite", f"{case}.wav"))
    print(f"{case}: {os.path.join(work, case + '.wav')}")


def main():
    wanted = sys.argv[1:]
    for case, program, frames, events in CASES:
        if not wanted or case in wanted:
            run(case, program, frames, events)
    for case, program, frames in SOUND:
        if not wanted or case in wanted:
            sound(case, program, frames)
    for case, program, frames in POKEY_SOUND:
        if not wanted or case in wanted:
            pokey_sound(case, program, frames)
    for case, program, frames in YM_SOUND:
        if not wanted or case in wanted:
            ym_sound(case, program, frames)


if __name__ == "__main__":
    main()
