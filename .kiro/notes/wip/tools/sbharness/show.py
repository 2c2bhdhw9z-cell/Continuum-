#!/usr/bin/env python3
"""Scratch: print SameBoy's picture against a reference as shade digits for chosen lines.

usage: show.py dmg|dmgb|cgbc name [first_line [last_line]]
"""
import sys
from mb import png_rgb, run_sb, ROOT
import os

model, name = sys.argv[1], sys.argv[2]
a = int(sys.argv[3]) if len(sys.argv) > 3 else 0
b = int(sys.argv[4]) if len(sys.argv) > 4 else 143
folder = {"dmg": "DMG-blob", "dmgb": "DMG-CPU B", "cgbc": "CPU CGB C", "cgbd": "CPU CGB D"}[model]
want = png_rgb(os.path.join(ROOT, "expected", folder, name + ".png"))
got, log = run_sb({"dmgb": "dmg"}.get(model, model), name, log=os.environ.get("SB_LOG") is not None)
colours = {}


def digit(p):
    greys = {(255, 255, 255): "0", (170, 170, 170): "1", (85, 85, 85): "2", (0, 0, 0): "3"}
    if p in greys:
        return greys[p]
    colours.setdefault(p, chr(ord("a") + len(colours)))
    return colours[p]


for y in range(a, b + 1):
    g = "".join(digit(got[y * 160 + x]) for x in range(160))
    w = "".join(digit(want[y * 160 + x]) for x in range(160))
    d = "".join("." if got[y * 160 + x] == want[y * 160 + x] else "X" for x in range(160))
    print(f"{y:3} sb   {g}")
    print(f"{y:3} want {w}")
    print(f"{y:3} diff {d}")
if colours:
    print("colours:", {v: k for k, v in colours.items()})
if log:
    print(log)
