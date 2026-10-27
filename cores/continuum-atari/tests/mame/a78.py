#!/usr/bin/env python3
"""Shared helpers for the scripts that make cAtari's MAME references: wrapping our own test
programs into .a78 cartridges, and running MAME 0.289's a7800 on them with our stand-in BIOS
(test-programs/bios.asm) under the BIOS's file name. Nothing MAME makes is committed except the
hashes and numbers the scripts write into tests/references/."""

import os
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.normpath(os.path.join(HERE, "..", ".."))
PROGRAMS = os.path.join(CRATE, "test-programs")


def a78(rom, mapper=0, title="cAtari test", pal=False, controllers=(1, 1), save=0):
    """An .a78 file: the 128-byte header the 7800 community documents, then the ROM."""
    h = bytearray(128)
    h[0] = 3
    h[1:10] = b"ATARI7800"
    t = title.encode()[:32]
    h[17:17 + len(t)] = t
    h[49:53] = len(rom).to_bytes(4, "big")
    h[53:55] = mapper.to_bytes(2, "big")
    h[55], h[56] = controllers
    h[57] = 1 if pal else 0
    h[58] = save
    h[100:128] = b"ACTUAL CART DATA STARTS HERE"
    return bytes(h) + rom


def program(name):
    return open(os.path.join(PROGRAMS, name + ".bin"), "rb").read()


def run_mame(work, cart, lua, seconds, extra=(), pal=False):
    """Runs MAME's a7800 (a7800p for PAL) in `work` with the cartridge and a Lua script; returns
    MAME's log. The stand-in BIOS goes in under the real BIOS's name: MAME runs it after warning
    that its checksum is wrong."""
    mame = os.environ["MAME"]
    machine = "a7800p" if pal else "a7800"
    roms = os.path.join(work, "roms", machine)
    os.makedirs(roms, exist_ok=True)
    bios = program("bios")
    if pal:
        # The PAL console's BIOS is 16 KB at $C000; ours at the top of it.
        open(os.path.join(roms, "c300558-001b.u7"), "wb").write(bytes([0xFF]) * 0x3000 + bios)
    else:
        open(os.path.join(roms, "7800.u7"), "wb").write(bios)
    cart_path = os.path.join(work, "cart.a78")
    open(cart_path, "wb").write(cart)
    lua_path = os.path.join(work, "run.lua")
    open(lua_path, "w").write(lua)
    env = dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
    out = subprocess.run(
        [mame, machine, "-rompath", os.path.join(work, "roms"), "-cart", cart_path,
         "-video", "none", "-sound", "none", "-nothrottle", "-skip_gameinfo",
         "-cfg_directory", os.path.join(work, "cfg"), "-nvram_directory", os.path.join(work, "nvram"),
         "-snapshot_directory", os.path.join(work, "snap"), "-seconds_to_run", str(seconds),
         "-autoboot_script", lua_path, *extra],
        cwd=work, env=env, capture_output=True, text=True,
    )
    log = out.stdout + out.stderr
    open(os.path.join(work, "mame.log"), "w").write(log)
    if "LUA ERROR" in log or "error" in log.lower() and "checksum" not in log.lower():
        pass
    return log
