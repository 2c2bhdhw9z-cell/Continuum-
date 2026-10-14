#!/usr/bin/env python3
"""Makes the references for tests/mcs48_mame.rs: each 8035 program here, run on MAME's Donkey
Kong sound board, traced instruction by instruction.

For each program (alu, flow, timer, io) it assembles the program and dkmain.asm with The
Macro Assembler AS, builds a "dkong" set out of our own files only (dkmain for the Z80, the
program as the sound ROM, the tune ROM pattern below, zeros for the rest; MAME runs it after
warning that the checksums are wrong), runs MAME with its debugger tracing the sound CPU from
its first instruction, and writes <program>.mame: how many instructions are compared, and an
FNV-1a hash of each 1024 trace lines. Nothing MAME made is committed but those hashes.

    MAME=.../arcaderef ASL=.../asl P2BIN=.../p2bin python3 mame_trace.py [program ...]

Run it from a scratch directory: MAME writes cfg/ and the like where it runs.
"""

import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PROGRAMS = ["alu", "flow", "timer", "io"]
BLOCK = 1024
LINE = re.compile(
    r"A=(..) F=(..) R=(.{16}) T=(..) P=(..) P1=(..) P2=(..) C=(\d+) (\d):([0-9A-F]{3}): (.*)"
)


def tune_rom():
    """The sound board's second ROM (MOVX reads it): a pattern of our own, the same as in
    tests/mcs48_mame.rs."""
    return bytes((i * 37 + (i >> 8) * 101 + 11) & 0xFF for i in range(2048))


def assemble(name, cpu, size):
    """AS's binary of tests/mcs48/<name>.asm, padded with zeros to the ROM's size."""
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, name + ".asm")
        with open(os.path.join(HERE, name + ".asm")) as f, open(src, "w") as g:
            g.write(f.read())
        subprocess.run([os.environ["ASL"], "-q", "-cpu", cpu, src], cwd=tmp, check=True)
        out = os.path.join(tmp, name + ".bin")
        subprocess.run(
            [os.environ["P2BIN"], "-q", "-l", "0", os.path.join(tmp, name + ".p"), out],
            cwd=tmp,
            check=True,
        )
        data = open(out, "rb").read()
    assert len(data) <= size, f"{name} is {len(data)} bytes"
    return data + bytes(size - len(data))


def build_set(romdir, program):
    d = os.path.join(romdir, "dkong")
    os.makedirs(d, exist_ok=True)
    files = {
        "c_5et_g.bin": assemble("dkmain", "z80", 4096),
        "c_5ct_g.bin": bytes(4096),
        "c_5bt_g.bin": bytes(4096),
        "c_5at_g.bin": bytes(4096),
        "s_3i_b.bin": program,
        "s_3j_b.bin": tune_rom(),
    }
    for name in ["v_5h_b.bin", "v_3pt.bin", "l_4m_b.bin", "l_4n_b.bin", "l_4r_b.bin", "l_4s_b.bin"]:
        files[name] = bytes(2048)
    for name in ["c-2k.bpr", "c-2j.bpr", "v-5e.bpr"]:
        files[name] = bytes(256)
    for name, data in files.items():
        with open(os.path.join(d, name), "wb") as f:
            f.write(data)


def trace(name):
    program = assemble(name, "8048", 2048)
    with open(os.path.join(HERE, name + ".bin"), "wb") as f:
        f.write(program)
    work = os.path.abspath(f"mcs48-{name}")
    os.makedirs(work, exist_ok=True)
    build_set(os.path.join(work, "roms"), program)
    log = os.path.join(work, "trace.log")
    lua = os.path.join(work, "trace.lua")
    with open(lua, "w") as f:
        f.write(
            "local dbg = manager.machine.debugger\n"
            "dbg:command('focus soundcpu')\n"
            'dbg.visible_cpu = manager.machine.devices[":soundcpu"]\n'
            f"dbg:command('trace {log},soundcpu,noloop,{{tracelog \"A=%02X F=%02X "
            "R=%02X%02X%02X%02X%02X%02X%02X%02X T=%02X P=%02X P1=%02X P2=%02X C=%d \","
            "a,curflags,r0,r1,r2,r3,r4,r5,r6,r7,tc,tpre,p1,p2,totalcycles}')\n"
        )
    env = dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
    subprocess.run(
        [os.environ["MAME"], "dkong", "-rompath", os.path.join(work, "roms"), "-video", "none",
         "-sound", "none", "-nothrottle", "-skip_gameinfo", "-seconds_to_run", "2", "-debug",
         "-debugger", "none", "-autoboot_script", lua],
        cwd=work, env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    lines = []
    with open(log) as f:
        for raw in f:
            # MAME notes each interrupt between blank lines; the jump to the vector shows anyway.
            if not raw.strip() or raw.strip().startswith("(interrupted"):
                continue
            m = LINE.match(raw)
            assert m, f"{name}: no state on {raw!r}"
            a, flags, r, t, p, p1, p2, cycles, bank, pc, text = m.groups()
            lines.append(
                (f"{bank}:{pc} A={a} F={flags} R={r} T={t} P={p} P1={p1} P2={p2} C={cycles}", text)
            )
            # Stop once the program sits in its last loop: a JMP to itself, three times over.
            if (
                len(lines) >= 3
                and text.startswith("jmp")
                and lines[-1][0][:5] == lines[-2][0][:5] == lines[-3][0][:5]
            ):
                break
    os.remove(log)
    if os.environ.get("MCS48_KEEP"):
        # MAME's trace, line for line as tests/mcs48_mame.rs writes ours with MCS48_TRACE.
        with open(os.path.join(work, f"{name}.trace"), "w") as f:
            f.write("\n".join(state for state, _ in lines))
        with open(os.path.join(work, f"{name}.dis"), "w") as f:
            f.write("\n".join(f"{state}  {text.strip()}" for state, text in lines))
    blocks = []
    for start in range(0, len(lines), BLOCK):
        h = 0xCBF29CE484222325
        for state, _ in lines[start:start + BLOCK]:
            for byte in (state + "\n").encode():
                h = ((h ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
        blocks.append(h)
    with open(os.path.join(HERE, name + ".mame"), "w") as f:
        f.write(
            f"# MAME 0.289's trace of {name}.bin on Donkey Kong's sound CPU (an MB8884, its 8035),\n"
            f"# made by mame_trace.py: the instructions compared, then an FNV-1a 64 hash of each\n"
            f"# {BLOCK} trace lines. First and last line:\n"
            f"# {lines[0][0]}\n# {lines[-1][0]}\n"
            f"instructions {len(lines)}\n"
        )
        for h in blocks:
            f.write(f"{h:016x}\n")
    print(f"{name}: {len(lines)} instructions, last {lines[-1][0]} ({lines[-1][1].strip()})")


if __name__ == "__main__":
    for program in sys.argv[1:] or PROGRAMS:
        trace(program)
