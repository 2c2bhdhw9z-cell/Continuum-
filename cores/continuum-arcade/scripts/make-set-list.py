#!/usr/bin/env python3
"""Writes cArcade's built-in set list, src/set_list.rs, from a MAME source tree and its -listxml.

    scripts/make-set-list.py <mame source dir> <mame binary>

What it takes from MAME is data, not code: each set's short name, title, year, maker and parent,
and for each ROM chip its file name, size, CRC32 and where its bytes load (MAME's ROM_START
blocks, which -listxml can't fully describe: ROM_CONTINUE, ROM_IGNORE, ROM_RELOAD, ROM_FILL and
ROM_COPY). The two are cross-checked: every chip the ROM_START block names must be the chip
-listxml lists, by name, size and CRC.

Which sets go in, and on which board layout, is decided below (BOARDS), set by set, from how
MAME's driver runs each one: only sets that run on exactly the hardware cArcade models. A board's
sets are only switched on in cArcade once the board is proven (.kiro/notes/docs/ARCADE_CORE.md).

Run it again whenever a board's list changes; the output is committed. Needs Python 3.8+.
"""

import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

# The Pac-Man board (src/pacman.rs). Each set: (MAME name, layout, DIP switch family).
#   plain   - the plain board: 16 KB of program at 0000h, A15 not wired (MAME's pacman machine)
#   pacplus - the same with Pac-Man Plus's decoder (init_pacplus)
#   mspac   - Ms. Pac-Man's daughterboard (MAME's mspacman machine with init_mspacman)
#   upper   - bootlegs that wire A15: a second 16 KB at 8000h (MAME's woodpek machine)
# Left out, because cArcade doesn't model their hardware yet: sets that decrypt their program
# (init_eyes, init_mspacmbe, init_pengomc1, init_mspacii, init_mspackpls), the two bootlegs with
# blue and green swapped (woodpek_rbg), and every other machine in the driver (Crush Roller,
# Eyes, Ms. Pac-Man Champion Edition, ...).
PACMAN_SETS = [
    ("puckman", "plain", "pacman"), ("puckmanb", "plain", "pacman"), ("puckmanf", "plain", "pacman"),
    ("puckmanh", "plain", "pacman"), ("puckmanb3", "plain", "pacman"), ("pacman", "plain", "pacman"),
    ("pacmanso", "plain", "pacman"), ("pacmanvg", "plain", "pacman"), ("pacmanf", "plain", "pacman"),
    ("puckmod", "plain", "pacman"), ("pacmod", "plain", "pacman"), ("pacmanjpm", "plain", "pacman"),
    ("pacmanmr", "plain", "pacman"), ("pacmanpe", "plain", "comecome"), ("newpuc2", "plain", "newpuc2"),
    ("newpuc2b", "plain", "newpuc2"), ("newpuckx", "plain", "pacman"), ("pacheart", "plain", "pacman"),
    ("bucaner", "plain", "pacman"), ("bucanera", "plain", "pacman"), ("hangly", "plain", "pacman"),
    ("hangly2", "plain", "pacman"), ("hangly3", "plain", "pacman"), ("baracuda", "plain", "pacman"),
    ("popeyeman", "plain", "pacman"), ("pacuman", "plain", "pacuman"), ("crockman", "plain", "pacman"),
    ("crockmnf", "plain", "pacman"), ("puckmana", "plain", "pacman"), ("joyman", "plain", "pacman"),
    ("ctrpllrp", "plain", "pacman"), ("piranhah", "plain", "mspacman"), ("pacmanfm", "plain", "pacman"),
    ("pacmanug", "plain", "pacman"), ("pacmansp", "plain", "pacmansp"),
    ("pacplus", "pacplus", "pacman"),
    ("mspacman", "mspac", "mspacman"), ("mspacmnf", "mspac", "mspacman"), ("mspacmat", "mspac", "mspacman"),
    ("msheartb", "mspac", "mspacman"), ("pacgal2", "mspac", "mspacman"), ("mspacmancr", "mspac", "mspacman"),
    ("mspacmab", "upper", "mspacman"), ("mspacmab2", "upper", "mspacman"), ("mspacmab4", "upper", "mspacman"),
    ("mspacmab6", "upper", "mspacman"), ("mspacmbmc", "upper", "mspacman"), ("mspacmanlai", "upper", "mspacman"),
    ("mspacmane", "upper", "mspacman"), ("mspacmane2", "upper", "mspacman"), ("pacgal", "upper", "mspacman"),
    ("mspacpls", "upper", "mspacman"), ("mspacmanhnc", "upper", "mspacman"), ("mspacmanbg", "upper", "mspacman"),
    ("mspacmanbg2", "upper", "mspacman"), ("mspacmanbgd", "upper", "mspacman"),
    ("mspacmanblt", "upper", "mspacman"), ("mspacmanblt2", "upper", "mspacman"),
    ("mspacmanblt3", "upper", "mspacman"), ("mspacmanblt4", "upper", "mspacman"),
    ("mspacmanbcc", "upper", "mspacman"), ("mspacmanbhe", "upper", "mspacman"),
    ("mspacmanbi", "upper", "mspacman"), ("mspacmanbgc", "upper", "mspacman"),
    ("mspacmanbpl", "upper", "mspacman"),
]

# Per board: its driver file, the Rust names, the regions the board reads (a chip loaded only
# elsewhere, like a PAL dump, is kept in the list but never required), and parts of a region it
# doesn't read (Pac-Man's 3M timing PROM, at namco 100h).
BOARDS = [
    {
        "driver": "src/mame/pacman/pacman.cpp",
        "sets": PACMAN_SETS,
        "layout": {"plain": "Board::Pacman(Variant::Plain)", "pacplus": "Board::Pacman(Variant::PacPlus)",
                   "mspac": "Board::Pacman(Variant::MsPacman)", "upper": "Board::Pacman(Variant::UpperRom)"},
        "dips": {"pacman": "Dips::Pacman", "mspacman": "Dips::MsPacman", "comecome": "Dips::ComeCome",
                 "newpuc2": "Dips::Newpuc2", "pacuman": "Dips::Pacuman", "pacmansp": "Dips::PacmanSp"},
        "reads": {"maincpu": None, "gfx1": (0, 0x2000), "proms": None, "namco": (0, 0x100)},
    },
]


def rs(v):
    """A Rust string literal."""
    return '"' + v.replace("\\", "\\\\").replace('"', '\\"') + '"'


def opt(v):
    """A Rust Option<&str> literal."""
    return f"Some({rs(v)})" if v else "None"


def fail(msg):
    sys.exit(f"make-set-list: {msg}")


def num(s):
    s = s.strip()
    return int(s, 0)


def split_args(s):
    """Splits a macro's arguments at top-level commas."""
    out, depth, cur, quote = [], 0, "", False
    for c in s:
        if c == '"':
            quote = not quote
        if not quote and c == "(":
            depth += 1
        if not quote and c == ")":
            depth -= 1
        if not quote and depth == 0 and c == ",":
            out.append(cur.strip())
            cur = ""
        else:
            cur += c
    if cur.strip():
        out.append(cur.strip())
    return out


def macros(body):
    """The ROM_* macros of a ROM_START block, in order, with their arguments."""
    body = re.sub(r"//[^\n]*", "", body)
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    out = []
    i = 0
    while True:
        m = re.compile(r"\b(ROM_[A-Z0-9_]+|ROMX_LOAD|ROMREGION_[A-Z0-9_]+)\s*\(").search(body, i)
        if not m:
            break
        depth, j = 0, m.end() - 1
        while True:
            if body[j] == "(":
                depth += 1
            elif body[j] == ")":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        out.append((m.group(1), split_args(body[m.end():j])))
        i = j + 1
    return out


def parse_block(name, body):
    regions, roms = [], []
    region, rom = None, None
    for mac, args in macros(body):
        if mac == "ROM_REGION":
            size, tag, flags = num(args[0]), args[1].strip('"'), args[2]
            erase = 0
            if "ROMREGION_ERASEFF" in flags:
                erase = 0xFF
            elif "ROMREGION_ERASEVAL" in flags:
                erase = num(re.search(r"ROMREGION_ERASEVAL\((\w+)\)", flags).group(1))
            elif flags not in ("0", "ROMREGION_ERASE00"):
                fail(f"{name}: region {tag} has flags {flags}, which this script doesn't follow")
            region = {"tag": tag, "size": size, "erase": erase, "ops": []}
            regions.append(region)
            rom = None
        elif mac == "ROM_LOAD":
            fname, at, length, hashes = args[0].strip('"'), num(args[1]), num(args[2]), args[3]
            status = "Good"
            if "NO_DUMP" in hashes:
                fail(f"{name}: {fname} was never dumped")
            if "BAD_DUMP" in hashes:
                status = "BadDump"
            crc = int(re.search(r"CRC\(([0-9a-fA-F]{8})\)", hashes).group(1), 16)
            rom = {"name": fname, "crc": crc, "status": status, "pos": 0, "index": len(roms)}
            roms.append(rom)
            region["ops"].append(("load", rom["index"], at, 0, length))
            rom["pos"] = length
        elif mac == "ROM_CONTINUE":
            at, length = num(args[0]), num(args[1])
            region["ops"].append(("load", rom["index"], at, rom["pos"], length))
            rom["pos"] += length
        elif mac == "ROM_IGNORE":
            rom["pos"] += num(args[0])
        elif mac == "ROM_RELOAD":
            at, length = num(args[0]), num(args[1])
            region["ops"].append(("load", rom["index"], at, 0, length))
        elif mac == "ROM_FILL":
            region["ops"].append(("fill", num(args[0]), num(args[1]), num(args[2])))
            rom = None
        elif mac == "ROM_COPY":
            src, src_at, at, length = args[0].strip('"'), num(args[1]), num(args[2]), num(args[3])
            tags = [r["tag"] for r in regions]
            if src not in tags:
                fail(f"{name}: ROM_COPY from {src}, which isn't loaded before it")
            region["ops"].append(("copy", tags.index(src), src_at, at, length))
            rom = None
        else:
            fail(f"{name}: {mac} isn't something this script follows")
    for r in roms:
        r["size"] = r["pos"]
    return regions, roms


def main():
    if len(sys.argv) != 3:
        fail(__doc__)
    src_dir, mame = Path(sys.argv[1]), sys.argv[2]
    xml = subprocess.run([mame, "-listxml"], capture_output=True, check=True,
                         env={"SDL_VIDEODRIVER": "dummy", "SDL_AUDIODRIVER": "dummy"}).stdout
    machines = {m.get("name"): m for m in ET.fromstring(xml).findall("machine")}
    version = subprocess.run([mame, "-version"], capture_output=True, text=True,
                             env={"SDL_VIDEODRIVER": "dummy", "SDL_AUDIODRIVER": "dummy"}).stdout.split()[0]

    out = [
        "// cArcade's built-in set list. GENERATED by scripts/make-set-list.py from MAME "
        + version + "'s",
        "// driver sources and -listxml: each set's name, title, parent, and each ROM chip's file name, size,",
        "// CRC32 and where its bytes load. Facts about the chips, so a zip can be recognised; no MAME code.",
        "// Don't edit by hand: change the script's lists and run it again.",
        "",
        "use crate::pacman::Variant;",
        "use crate::sets::{Board, Dips, Op, Region, Rom, SetSpec, Status};",
        "",
        "pub static SETS: &[SetSpec] = &[",
    ]
    for board in BOARDS:
        text = (src_dir / board["driver"]).read_text()
        blocks = {m.group(1): m.group(2)
                  for m in re.finditer(r"ROM_START\(\s*(\w+)\s*\)(.*?)ROM_END", text, re.S)}
        for name, layout, dips in board["sets"]:
            mach = machines.get(name)
            if mach is None:
                fail(f"{name} isn't in this MAME")
            regions, roms = parse_block(name, blocks[name])
            xroms = mach.findall("rom")
            listed = sorted((r.get("name"), int(r.get("size")), int(r.get("crc"), 16)) for r in xroms)
            parsed = sorted((r["name"], r["size"], r["crc"]) for r in roms)
            if listed != parsed:
                fail(f"{name}: its ROM_START block and -listxml disagree:\n{parsed}\n{listed}")
            merge = {r.get("name"): r.get("merge") for r in xroms}
            # Which chips the board reads: a load into a region it reads, inside the part it reads.
            used = set()
            for reg in regions:
                part = board["reads"].get(reg["tag"], "no")
                for op in reg["ops"]:
                    if op[0] != "load" or part == "no":
                        continue
                    _, idx, at, _, length = op
                    if part is None or (at < part[1] and at + length > part[0]):
                        used.add(idx)
            # A ROM_COPY's source region counts as read when it copies into one that is.
            for reg in regions:
                if reg["tag"] in board["reads"]:
                    for op in reg["ops"]:
                        if op[0] == "copy":
                            src = regions[op[1]]
                            for sop in src["ops"]:
                                if sop[0] == "load":
                                    used.add(sop[1])
            parent = mach.get("cloneof")
            romof = mach.get("romof")
            out.append("    SetSpec {")
            out.append(f'        name: "{name}",')
            out.append(f'        title: {rs(mach.find("description").text)},')
            out.append(f'        year: {rs(mach.find("year").text)},')
            out.append(f'        maker: {rs(mach.find("manufacturer").text)},')
            out.append(f"        parent: {opt(parent)},")
            out.append(f"        romof: {opt(romof)},")
            out.append(f"        board: {board['layout'][layout]},")
            out.append(f"        dips: {board['dips'][dips]},")
            # MAME's factory setting of every DIP switch and configuration switch, per port: the
            # bits they cover and what they're set to, so a test can hold cArcade's defaults to it.
            factory = {}
            for d in mach.findall("dipswitch") + mach.findall("configuration"):
                tag = d.get("tag").lstrip(":")
                mask = int(d.get("mask"))
                kids = d.findall("dipvalue") + d.findall("confsetting")
                dflt = [int(v.get("value")) for v in kids if v.get("default") == "yes"]
                if len(dflt) != 1:
                    fail(f"{name}: {d.get('name')} has no single default")
                m, v = factory.get(tag, (0, 0))
                factory[tag] = (m | mask, v | (dflt[0] & mask))
            fac = ", ".join(f'("{t}", {m:#04x}, {v:#04x})' for t, (m, v) in sorted(factory.items()))
            out.append(f"        factory: &[{fac}],")
            out.append("        roms: &[")
            for r in roms:
                m = merge.get(r["name"])
                out.append(
                    f'            Rom {{ name: "{r["name"]}", size: {r["size"]:#x}, crc: 0x{r["crc"]:08x}, '
                    f"merge: {opt(m)}, status: Status::{r['status']}, "
                    f"needed: {str(r['index'] in used).lower()} }},")
            out.append("        ],")
            out.append("        regions: &[")
            for reg in regions:
                ops = []
                for op in reg["ops"]:
                    if op[0] == "load":
                        ops.append(f"Op::Load {{ rom: {op[1]}, at: {op[2]:#x}, from: {op[3]:#x}, len: {op[4]:#x} }}")
                    elif op[0] == "fill":
                        ops.append(f"Op::Fill {{ at: {op[1]:#x}, len: {op[2]:#x}, value: {op[3]:#04x} }}")
                    else:
                        ops.append(f"Op::Copy {{ region: {op[1]}, from: {op[2]:#x}, at: {op[3]:#x}, len: {op[4]:#x} }}")
                out.append(f'            Region {{ tag: "{reg["tag"]}", size: {reg["size"]:#x}, erase: {reg["erase"]:#04x}, ops: &[')
                for o in ops:
                    out.append(f"                {o},")
                out.append("            ] },")
            out.append("        ],")
            out.append("    },")
    out.append("];")
    dest = Path(__file__).resolve().parent.parent / "src" / "set_list.rs"
    dest.write_text("\n".join(out) + "\n")
    print(f"wrote {dest}: {sum(len(b['sets']) for b in BOARDS)} sets")


if __name__ == "__main__":
    main()
