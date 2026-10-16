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

The Galaxian family (Galaxian, Moon Cresta and Konami's Scramble boards) and the Donkey Kong board
are chosen from the GAME() lines of MAME's galaxian.cpp and dkong.cpp instead (GALAXIAN_MACHINES and
DKONG_MACHINES below: the machine and init each of our board variants matches, and no set MAME
flags as not working or imperfect), and their sets' inputs and DIP switches are generated too: MAME is started on each set with zero-filled stand-ins for its
ROMs (no real ROM is needed or read) and a Lua script lists every input port's fields: which bit
is which control, active high or low, and what the port reads at rest. The switches' names and
settings come from -listxml, worded for players by SWITCH_WORDS below.

Run it again whenever a board's list changes; the output is committed. Needs Python 3.8+.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
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


# The Galaxian family: (machine, init) in MAME's GAME() lines -> our board variant. Only sets of the
# plain galaxian_state class (subclasses change the hardware) and without these flags go in.
GALAXIAN_MACHINES = {
    ("galaxian", "init_galaxian"): "Variant::Galaxian",
    ("mooncrst", "init_mooncrst"): "Variant::MoonCresta { encrypted: true }",
    ("mooncrst", "init_mooncrsu"): "Variant::MoonCresta { encrypted: false }",
    ("scramble", "init_scramble"): "Variant::Konami(Konami::Scramble)",
    ("theend", "init_theend"): "Variant::Konami(Konami::TheEnd)",
    ("scobra", "init_scobra"): "Variant::Konami(Konami::SuperCobra)",
    ("frogger", "init_frogger"): "Variant::Konami(Konami::Frogger)",
    ("turtles", "init_turtles"): "Variant::Konami(Konami::Turtles)",
}
GALAXIAN_BAD_FLAGS = ("NOT_WORKING", "WRONG_COLORS", "IMPERFECT", "UNEMULATED", "NO_SOUND", "NO_COCKTAIL")
GALAXIAN_REGIONS = {"maincpu", "gfx1", "proms", "audiocpu"}

# The Donkey Kong board (src/dkong.rs): (machine, init) in the GAME() lines of MAME's dkong.cpp ->
# our board variant, and the ROM regions that board has (tag: size). Only sets of the plain
# dkong_state class, without the flags above, whose regions are exactly these go in. Left out:
# Braze's kits (an extra decrypting chip), init_dkingjr's bootlegs (inverted colour PROMs),
# Junior King (16 KB of sprites), and the driver's other machines (Radar Scope, Donkey Kong 3,
# the S2650 conversions, ...).
DKONG_MACHINES = {
    ("dkong2b", "empty_init"): "DVariant::DonkeyKong",
    ("dkongjr", "empty_init"): "DVariant::DonkeyKongJr",
}
DKONG_REGIONS = {
    "DVariant::DonkeyKong": {"maincpu": 0x10000, "soundcpu": 0x1800, "gfx1": 0x1000, "gfx2": 0x2000, "proms": 0x300},
    "DVariant::DonkeyKongJr": {"maincpu": 0x10000, "soundcpu": 0x1000, "gfx1": 0x2000, "gfx2": 0x2000, "proms": 0x300},
}

# How a switch is worded for players: MAME's name -> our label, or None to leave it at the factory
# setting and not offer it (service and test switches, unknown ones).
SWITCH_WORDS = {
    "Lives": "Lives", "Bonus Life": "Extra life", "Coinage": "Coins", "Coin A": "Coins (left)",
    "Coin B": "Coins (right)", "Cabinet": "Cabinet", "Difficulty": "Difficulty",
    "Demo Sounds": "Sound in demo", "Free Play": "Free play", "Allow Continue": "Continue",
    "Game Time": "Time", "Language": "Language", "Players": "Players",
    "Service Mode": None, "Unknown": None, "Unused": None, "Flip Screen": None, "Test": None,
    "Freeze": None, "Debug": None, "Rack Test": None, "Invulnerability": None, "Infinite Lives": None,
}


def value_words(text):
    """A switch setting worded for players: '1 Coin/2 Credits' -> '1 coin, 2 games', numbers with
    commas, and the rest as MAME has it."""
    t = text.strip()
    m = re.fullmatch(r"(\d+) Coins?/(\d+) Credits?", t)
    if m:
        a, b = int(m.group(1)), int(m.group(2))
        return f"{a} coin{'s' if a > 1 else ''}, {b} game{'s' if b > 1 else ''}"
    if t == "Free Play":
        return "Free play"
    if t == "Cocktail":
        return "Cocktail table"
    if re.fullmatch(r"\d{4,}", t):
        return f"{int(t):,}"
    return re.sub(r"\s+", " ", t)


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


def galaxian_games(text):
    """The GAME() lines of galaxian.cpp: name, machine, init, class, rotation, flags."""
    out = []
    for m in re.finditer(r"^GAME\(\s*([^,]+),\s*(\w+),\s*(\w+),\s*(\w+),\s*(\w+),\s*(\w+),\s*(\w+),\s*(ROT\d+),"
                         r"\s*\"[^\"]*\",\s*\"[^\"]*\",\s*([^)]*)\)", text, re.M):
        _, name, _, mach, inp, cls, init, rot, flags = m.groups()
        out.append((name, mach, init, cls, rot, flags, inp))
    return out


def protection_bits(text, inp, seen=None):
    """The bits of IN2 an input definition wires to the protection chip (MAME's
    theend_protection_alt_r), following its PORT_INCLUDEs."""
    seen = seen or set()
    if inp in seen:
        return 0
    seen.add(inp)
    m = re.search(r"INPUT_PORTS_START\(\s*" + re.escape(inp) + r"\s*\)(.*?)INPUT_PORTS_END", text, re.S)
    if not m:
        return 0
    bits = 0
    for b in re.finditer(r"PORT_BIT\(\s*(0x[0-9a-fA-F]+)[^)]*\)\s*PORT_READ_LINE_MEMBER\(FUNC\(galaxian_state::theend_protection_alt_r", m.group(1)):
        bits |= int(b.group(1), 16)
    for inc in re.findall(r"PORT_INCLUDE\(\s*(\w+)\s*\)", m.group(1)):
        bits |= protection_bits(text, inc, seen)
    return bits


DUMP_LUA = """
if _G.done then return end
_G.done = true
local f = assert(io.open('ports.txt', 'w'))
local io_ = manager.machine.ioport
for tag, port in pairs(io_.ports) do
  f:write(string.format('port %s %d\\n', tag, port:read()))
  for name, fld in pairs(port.fields) do
    f:write(string.format('field %s %d %d %s %s %d %d %s\\n', tag, fld.mask, fld.defvalue,
      io_:input_type_to_token(fld.type), tostring(fld.type_class), fld.player, fld.way or 0,
      tostring(fld.cocktail)))
  end
end
f:close()
manager.machine:exit()
"""


def dump_ports(mame, mach):
    """Starts MAME on the set with zero-filled stand-ins for its ROMs and lists its input ports:
    {tag: (value at rest, [(mask, defvalue, token, class, player, way, cocktail)])}."""
    name = mach.get("name")
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp) / "roms" / name
        d.mkdir(parents=True)
        for r in mach.findall("rom"):
            (d / r.get("name")).write_bytes(bytes(int(r.get("size"))))
        (Path(tmp) / "dump.lua").write_text(DUMP_LUA)
        env = dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
        subprocess.run([mame, name, "-rompath", str(Path(tmp) / "roms"), "-video", "none", "-sound", "none",
                        "-nothrottle", "-skip_gameinfo", "-autoboot_script", "dump.lua", "-seconds_to_run", "3",
                        "-cfg_directory", "cfg", "-nvram_directory", "nvram"],
                       cwd=tmp, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        f = Path(tmp) / "ports.txt"
        if not f.exists():
            fail(f"{name}: MAME didn't list its ports")
        ports = {}
        for line in f.read_text().splitlines():
            w = line.split()
            if w[0] == "port":
                ports.setdefault(w[1], [0, []])[0] = int(w[2])
            else:
                ports.setdefault(w[1], [0, []])[1].append(
                    (int(w[2]), int(w[3]), w[4], w[5], int(w[6]), int(w[7]), w[8] == "true"))
        return ports


CONTROL_TOKENS = {
    "JOYSTICK_UP": "Up", "JOYSTICK_DOWN": "Down", "JOYSTICK_LEFT": "Left", "JOYSTICK_RIGHT": "Right",
}


def galaxian_sets(src_dir, mame, machines, out, families):
    """Writes the Galaxian family's SetSpecs into `out`, and their switch families into `families`
    (shared between sets whose switches are the same)."""
    text = (src_dir / "src/mame/galaxian/galaxian.cpp").read_text()
    blocks = {m.group(1): m.group(2) for m in re.finditer(r"ROM_START\(\s*(\w+)\s*\)(.*?)ROM_END", text, re.S)}
    count = 0
    for name, mach_fn, init, cls, rot, flags, inp in galaxian_games(text):
        variant = GALAXIAN_MACHINES.get((mach_fn, init))
        if variant is None or cls != "galaxian_state" or any(b in flags for b in GALAXIAN_BAD_FLAGS):
            continue
        mach = machines.get(name)
        if mach is None or name not in blocks:
            print(f"  skipped {name}: not in this MAME")
            continue
        try:
            regions, roms = parse_block(name, blocks[name])
        except SystemExit as e:
            print(f"  skipped {name}: {e}")
            continue
        tags = {r["tag"] for r in regions}
        sizes = {r["tag"]: r["size"] for r in regions}
        konami = "Konami" in variant
        if not tags <= GALAXIAN_REGIONS or "gfx1" not in tags or sizes["gfx1"] not in (0x1000, 0x2000) \
                or ("audiocpu" in tags) != konami or sizes.get("proms") != 0x20:
            print(f"  skipped {name}: regions {sorted(sizes.items())} aren't the board's")
            continue
        xroms = mach.findall("rom")
        listed = sorted((r.get("name"), int(r.get("size")), int(r.get("crc"), 16)) for r in xroms if r.get("crc"))
        parsed = sorted((r["name"], r["size"], r["crc"]) for r in roms)
        if listed != parsed:
            fail(f"{name}: its ROM_START block and -listxml disagree:\n{parsed}\n{listed}")
        protection = protection_bits(text, inp) if ("Scramble" in variant or "TheEnd" in variant) else 0
        rotation = "Rotation::Rot270" if rot == "ROT270" else "Rotation::Rot90"
        emit_generated(mame, mach, regions, roms, f"Board::Galaxian({variant}, {rotation}, {protection:#04x})",
                       [":IN0", ":IN1", ":IN2", ":IN3"], out, families)
        count += 1
    return count


def emit_generated(mame, mach, regions, roms, board, tags_in, out, families):
    """Writes one generated set's SetSpec into `out`: its inputs from MAME's port lists (ports
    `tags_in`, in the board's order), its switches from -listxml (sharing a family in `families`
    with sets whose switches are the same), its chips and regions."""
    name = mach.get("name")
    xroms = mach.findall("rom")
    ports = dump_ports(mame, mach)
    rest = [ports.get(t, [0, []])[0] & 0xFF for t in tags_in]
    inputs, four_way = [], False
    # In bit order: MAME's Lua lists a port's fields in no fixed order.
    for pi, t in enumerate(tags_in):
        for mask, dflt, token, klass, player, way, cocktail in sorted(ports.get(t, [0, []])[1]):
            token = re.sub(r"^P\d_", "", token)
            active_high = dflt == 0
            ctl = None
            if token in CONTROL_TOKENS:
                ctl = f"Control::{CONTROL_TOKENS[token]}({player})"
                four_way |= way == 4
            elif re.fullmatch(r"BUTTON(\d)", token):
                ctl = f"Control::Button({player}, {int(token[6:]) - 1})"
            elif re.fullmatch(r"COIN(\d)", token):
                ctl = f"Control::Coin({int(token[4:]) - 1})"
            elif re.fullmatch(r"START(\d)", token):
                ctl = f"Control::Start({int(token[5:]) - 1})"
            if ctl and mask < 0x100:
                inputs.append(f"InputBit {{ port: {pi}, mask: {mask:#04x}, active_high: {str(active_high).lower()}, control: {ctl} }}")
    # The switches, as -listxml has them, worded for players.
    switches = []
    for d in mach.findall("dipswitch"):
        tag = d.get("tag").lstrip(":")
        words = SWITCH_WORDS.get(d.get("name"), d.get("name") if len(d.get("name")) <= 20 else None)
        if words is None or f":{tag}" not in tags_in:
            continue
        values = [(int(v.get("value")), value_words(v.get("name")), v.get("default") == "yes") for v in d.findall("dipvalue")]
        switches.append((tags_in.index(f":{tag}"), int(d.get("mask")), words, tuple(values)))
    sig = tuple(switches)
    fam = next((i for i, f in enumerate(families) if f["sig"] == sig), None)
    if fam is None:
        fam = len(families)
        families.append({"sig": sig, "key": name, "title": mach.find("description").text.split(" (")[0]})
    factory = {}
    for d in mach.findall("dipswitch"):
        t = d.get("tag").lstrip(":")
        m = int(d.get("mask"))
        dv = [int(v.get("value")) for v in d.findall("dipvalue") if v.get("default") == "yes"]
        fm, fv = factory.get(t, (0, 0))
        factory[t] = (fm | m, fv | (dv[0] & m))
    fac = ", ".join(f'("{t}", {m:#04x}, {v:#04x})' for t, (m, v) in sorted(factory.items()))
    merge = {r.get("name"): r.get("merge") for r in xroms}
    used = {r["index"] for r in roms}
    out.append("    SetSpec {")
    out.append(f'        name: "{name}",')
    out.append(f'        title: {rs(mach.find("description").text)},')
    out.append(f'        year: {rs(mach.find("year").text)},')
    out.append(f'        maker: {rs(mach.find("manufacturer").text)},')
    out.append(f"        parent: {opt(mach.get('cloneof'))},")
    out.append(f"        romof: {opt(mach.get('romof'))},")
    out.append(f"        board: {board},")
    out.append(f"        dips: Dips::Table({fam}),")
    out.append(f"        factory: &[{fac}],")
    out.append(f"        inputs: &[{', '.join(inputs)}],")
    out.append(f"        rest: [{', '.join(f'{v:#04x}' for v in rest)}],")
    out.append(f"        four_way: {str(four_way).lower()},")
    out.append("        roms: &[")
    for r in roms:
        out.append(
            f'            Rom {{ name: "{r["name"]}", size: {r["size"]:#x}, crc: 0x{r["crc"]:08x}, '
            f"merge: {opt(merge.get(r['name']))}, status: Status::{r['status']}, "
            f"needed: {str(r['index'] in used).lower()} }},")
    out.append("        ],")
    out.append("        regions: &[")
    for reg in regions:
        out.append(f'            Region {{ tag: "{reg["tag"]}", size: {reg["size"]:#x}, erase: {reg["erase"]:#04x}, ops: &[')
        for op in reg["ops"]:
            if op[0] == "load":
                out.append(f"                Op::Load {{ rom: {op[1]}, at: {op[2]:#x}, from: {op[3]:#x}, len: {op[4]:#x} }},")
            elif op[0] == "fill":
                out.append(f"                Op::Fill {{ at: {op[1]:#x}, len: {op[2]:#x}, value: {op[3]:#04x} }},")
            else:
                out.append(f"                Op::Copy {{ region: {op[1]}, from: {op[2]:#x}, at: {op[3]:#x}, len: {op[4]:#x} }},")
        out.append("            ] },")
    out.append("        ],")
    out.append("    },")


def dkong_sets(src_dir, mame, machines, out, families):
    """Writes the Donkey Kong board's SetSpecs into `out` (their switches into `families`), chosen
    as DKONG_MACHINES says."""
    text = (src_dir / "src/mame/nintendo/dkong.cpp").read_text()
    blocks = {m.group(1): m.group(2) for m in re.finditer(r"ROM_START\(\s*(\w+)\s*\)(.*?)ROM_END", text, re.S)}
    count = 0
    for name, mach_fn, init, cls, rot, flags, inp in galaxian_games(text):
        variant = DKONG_MACHINES.get((mach_fn, init))
        if variant is None or cls != "dkong_state" or any(b in flags for b in GALAXIAN_BAD_FLAGS):
            continue
        mach = machines.get(name)
        if mach is None or name not in blocks:
            print(f"  skipped {name}: not in this MAME")
            continue
        regions, roms = parse_block(name, blocks[name])
        sizes = {r["tag"]: r["size"] for r in regions}
        if sizes != DKONG_REGIONS[variant] or any(r["erase"] for r in regions):
            print(f"  skipped {name}: regions {sorted(sizes.items())} aren't the board's")
            continue
        xroms = mach.findall("rom")
        listed = sorted((r.get("name"), int(r.get("size")), int(r.get("crc"), 16)) for r in xroms if r.get("crc"))
        parsed = sorted((r["name"], r["size"], r["crc"]) for r in roms)
        if listed != parsed:
            fail(f"{name}: its ROM_START block and -listxml disagree:\n{parsed}\n{listed}")
        emit_generated(mame, mach, regions, roms, f"Board::Dkong({variant})", [":IN0", ":IN1", ":IN2", ":DSW0"],
                       out, families)
        count += 1
    return count


def cstr(v):
    return 'c"' + v.replace("\\", "\\\\").replace('"', '\\"') + '"'


def slug(v):
    return re.sub(r"[^a-z0-9]+", "_", v.lower()).strip("_")


def write_families(families, out):
    """The Galaxian family's switches as core options: one group per family of sets with the same
    switches, keyed carcade_<first set>_<switch>."""
    out.append("")
    out.append("/// The switches of the sets generated from MAME's port lists, one family per set of switches.")
    out.append("pub static FAMILIES: &[SwitchFamily] = &[")
    for f in families:
        out.append("    SwitchFamily {")
        out.append(f"        key: {cstr(f['key'])},")
        out.append(f"        title: {cstr(f['title'] + ' games')},")
        out.append("        switches: &[")
        keys = set()
        for port, mask, words, values in f["sig"]:
            dflt = next((v for v in values if v[2]), values[0])
            key = "carcade_" + slug(f["key"]) + "_" + slug(words)
            if key in keys:
                key += f"_{port}_{mask:02x}"
            keys.add(key)
            out.append("            Switch {")
            out.append(f"                key: {cstr(key)},")
            out.append(f"                label: {cstr(words)},")
            out.append('                info: c"",')
            out.append(f"                category: {cstr(f['key'])},")
            out.append(f"                port: {port},")
            out.append(f"                mask: {mask:#04x},")
            vals = ", ".join(f"({cstr(format(v[0], '02x'))}, {cstr(v[1])}, {v[0]:#04x})" for v in values)
            out.append(f"                values: &[{vals}],")
            out.append(f"                default: {cstr(format(dflt[0], '02x'))},")
            out.append("            },")
        out.append("        ],")
        out.append("    },")
    out.append("];")


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
        "use crate::dkong::Variant as DVariant;",
        "use crate::galaxian::{Konami, Rotation, Variant as GVariant};",
        "use crate::options::{Switch, SwitchFamily};",
        "use crate::pacman::Variant;",
        "use crate::sets::{Board, Control, Dips, InputBit, Op, Region, Rom, SetSpec, Status};",
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
            out.append("        inputs: &[],")
            out.append("        rest: [0, 0, 0, 0],")
            out.append("        four_way: true,")
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
    families = []
    galaxian = galaxian_sets(src_dir, mame, machines, out, families)
    dkong = dkong_sets(src_dir, mame, machines, out, families)
    out.append("];")
    write_families(families, out)
    text = "\n".join(out) + "\n"
    # The Galaxian board's variant is GVariant here (Pac-Man's is Variant).
    text = text.replace("Board::Galaxian(Variant::", "Board::Galaxian(GVariant::")
    dest = Path(__file__).resolve().parent.parent / "src" / "set_list.rs"
    dest.write_text(text)
    print(f"wrote {dest}: {sum(len(b['sets']) for b in BOARDS)} Pac-Man board sets, {galaxian} Galaxian family sets, "
          f"{dkong} Donkey Kong board sets, {len(families)} switch families")


if __name__ == "__main__":
    main()
