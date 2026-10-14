#!/usr/bin/env python3
"""Scratch tool: run Mealybug ROMs on SameBoy (sbh) and compare with the references.

usage: mb.py dmg|cgbc [name-fragment]
"""
import os
import struct
import subprocess
import sys
import zlib

ROOT = "/projects/sandbox/.scratch/cboy-work/cores/continuum-gb/test-roms/mealybug"
SBH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sbh")
BOOT = "/projects/sandbox/.scratch/cboy-tmp/sameboy/build/bin/BootROMs"


def png_rgb(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    i = 8
    idat = b""
    plte = None
    while i < len(data):
        n, kind = struct.unpack(">I4s", data[i:i + 8])
        body = data[i + 8:i + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, inter = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            plte = [tuple(body[j:j + 3]) for j in range(0, n, 3)]
        elif kind == b"IDAT":
            idat += body
        i += 12 + n
    assert inter == 0
    raw = zlib.decompress(idat)
    chans = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    bpp_bits = chans * depth
    stride = (w * bpp_bits + 7) // 8
    bpp = max(1, bpp_bits // 8)
    rows = []
    prev = bytearray(stride)
    p = 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
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
        rows.append(line)
        prev = line
    out = []
    for line in rows:
        for x in range(w):
            if depth < 8:
                per = 8 // depth
                v = (line[x // per] >> (8 - depth * (x % per + 1))) & ((1 << depth) - 1)
                if ctype == 3:
                    out.append(plte[v])
                else:
                    g = v * 255 // ((1 << depth) - 1)
                    out.append((g, g, g))
            else:
                o = x * chans
                if ctype == 0:
                    out.append((line[o],) * 3)
                elif ctype == 3:
                    out.append(plte[line[o]])
                elif ctype == 4:
                    out.append((line[o],) * 3)
                else:
                    out.append(tuple(line[o:o + 3]))
    return out


def run_sb(model, name, log=False):
    boot = os.path.join(BOOT, "dmg_boot.bin" if model == "dmg" else "cgb_boot.bin")
    out = f"/tmp/sb_{model}_{name}.rgb"
    env = dict(os.environ)
    if log:
        env["SB_LOG"] = "1"
    r = subprocess.run([SBH, model, boot, os.path.join(ROOT, name + ".gb"), out], env=env,
                       capture_output=True, text=True)
    px = open(out, "rb").read()
    return [tuple(px[i:i + 3]) for i in range(0, len(px), 3)], r.stdout


def main():
    model = sys.argv[1]
    frag = sys.argv[2] if len(sys.argv) > 2 else ""
    folder = {"dmg": "DMG-blob", "dmgb": "DMG-CPU B", "cgbc": "CPU CGB C", "cgbd": "CPU CGB D"}[model]
    run_model = {"dmgb": "dmg"}.get(model, model)
    exp = os.path.join(ROOT, "expected", folder)
    passed = 0
    names = sorted(n[:-4] for n in os.listdir(exp) if n.endswith(".png") and frag in n)
    for n in names:
        want = png_rgb(os.path.join(exp, n + ".png"))
        got, _ = run_sb(run_model, n)
        diff = [(i % 160, i // 160) for i in range(160 * 144) if want[i] != got[i]]
        if not diff:
            passed += 1
            print(f"PASS {n}")
        else:
            lines = len(set(y for _, y in diff))
            x, y = diff[0]
            print(f"FAIL {n}: {len(diff)} px on {lines} lines, first ({x},{y}) got {got[y*160+x]} want {want[y*160+x]}")
    print(f"{model}: {passed} of {len(names)}")


if __name__ == "__main__":
    main()
