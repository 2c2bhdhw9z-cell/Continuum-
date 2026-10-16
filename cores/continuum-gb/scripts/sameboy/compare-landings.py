#!/usr/bin/env python3
"""Lines cBoy's exact drawer up against SameBoy's on every picture-register write of one frame of
each Mealybug ROM: the drawer's position, LCD x, fetcher step, object fetch, window and FIFO
length on the dot each write lands. cBoy's side comes from the `trace` feature
(tests/roms.rs::trace_landings); SameBoy's from harness.c (build.sh).

usage: compare-landings.py dmg|cgbc [names...]
"""
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CORE = os.path.abspath(os.path.join(HERE, "..", ".."))
WORK = os.environ.get("SAMEBOY_WORK", "/tmp/cboy-sameboy")
SBH = os.environ.get("SBH", os.path.join(WORK, "sbh"))
BOOT = os.path.join(WORK, "sameboy", "build", "bin", "BootROMs")
# SameBoy's display states that are mode 3: the pixel dot, the object fetch's, the WX=0 extra dot.
MODE3 = {21, 27, 41, 20, 39, 40, 42}
FIELDS = r"(\w+)=\s*(-?\w+)"


def sameboy(model, name):
    boot = os.path.join(BOOT, "dmg_boot.bin" if model == "dmg" else "cgb_boot.bin")
    r = subprocess.run([SBH, model, boot, f"{CORE}/test-roms/mealybug/{name}.gb", "/tmp/cboy-landings.rgb"],
                       env=dict(os.environ, SB_LOG="1"), capture_output=True, text=True)
    out = []
    for line in r.stdout.splitlines():
        if line.startswith("W "):
            d = dict(re.findall(FIELDS, line))
            reg, val = line.split()[1].split("=")
            out.append((reg, val, int(d["line"]), int(d["state"]) in MODE3, *(int(d[k]) for k in
                        ("pos", "lcd_x", "fetch", "during_obj", "wx_trig", "bgfifo"))))
    return out


def cboy(model, name):
    env = dict(os.environ, CBOY_ROM=name)
    if model != "dmg":
        env["CBOY_CGB"] = "1"
    r = subprocess.run(["cargo", "test", "--release", "-q", "--features", "trace", "--test", "roms",
                        "trace_landings", "--", "--nocapture"], cwd=CORE, env=env, capture_output=True, text=True)
    out = []
    for line in r.stderr.splitlines():
        if line.startswith("L "):
            d = dict(re.findall(FIELDS, line))
            reg, val = line.split()[1].split("=")
            out.append((reg, val, int(d["line"]), d["active"] == "true", *(int(d[k]) for k in
                        ("pos", "lcd_x", "fetch", "during_obj", "wx_trig", "bgfifo"))))
    return out


def main():
    model = sys.argv[1]
    folder = {"dmg": "DMG-blob", "cgbc": "CPU CGB C"}[model]
    names = sys.argv[2:] or sorted(n[:-4] for n in os.listdir(f"{CORE}/test-roms/mealybug/expected/{folder}"))
    totals = {}
    for n in names:
        s, c = sameboy(model, n), cboy(model, n)
        if len(s) != len(c):
            print(f"{n}: {len(s)} writes on SameBoy, {len(c)} on cBoy")
        first = {}
        for a, b in zip(s, c):
            if a[0] != b[0] or a[2] != b[2]:
                print(f"{n}: out of step at {a} / {b}")
                break
            if not (a[3] and b[3]):
                continue
            t = totals.setdefault(a[0], [0, 0])
            t[0] += 1
            if a[4:] != b[4:]:
                t[1] += 1
                first.setdefault(a[0], (a, b))
        for reg, (a, b) in first.items():
            print(f"{n} {reg} first difference:\n   SameBoy {a}\n   cBoy    {b}")
    for reg, (n, bad) in sorted(totals.items()):
        print(f"{reg}: {n} writes landing in mode 3, {bad} with a different drawer state")


if __name__ == "__main__":
    main()
