#!/usr/bin/env python3
"""Makes the reference for tests/ay8910_mame.rs: jumpbug.asm run on MAME's Jump Bug board, its
AY-3-8910 recorded.

It assembles jumpbug.asm with The Macro Assembler AS, builds a "jumpbug" set out of that and
zeros (MAME runs it after warning that the checksums are wrong), runs MAME with -wavwrite at
192,000 samples a second, which is the chip's own output rate on that board (1.536 MHz / 8), and
writes jumpbug.mame: MAME's output value for each fixed amplitude and each envelope level (from
the calibration at the start of the script), how many samples are compared, and an FNV-1a hash
of each 4096 of them. AY_KEEP=1 also keeps the samples, as ay-jumpbug/mame.raw (16-bit).

    MAME=.../arcaderef ASL=.../asl P2BIN=.../p2bin python3 mame_ay.py
"""

import os
import struct
import subprocess
import tempfile
import wave

HERE = os.path.dirname(os.path.abspath(__file__))
BLOCK = 4096


def assemble():
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "jumpbug.asm")
        with open(os.path.join(HERE, "jumpbug.asm")) as f, open(src, "w") as g:
            g.write(f.read())
        subprocess.run([os.environ["ASL"], "-q", "-cpu", "z80", src], cwd=tmp, check=True)
        out = os.path.join(tmp, "jumpbug.bin")
        subprocess.run(
            [os.environ["P2BIN"], "-q", "-l", "0", os.path.join(tmp, "jumpbug.p"), out],
            cwd=tmp,
            check=True,
        )
        return open(out, "rb").read()


def runs(values):
    out = []
    for v in values:
        if out and out[-1][0] == v:
            out[-1][1] += 1
        else:
            out.append([v, 1])
    return out


def main():
    program = assemble()
    with open(os.path.join(HERE, "jumpbug.bin"), "wb") as f:
        f.write(program)
    work = os.path.abspath("ay-jumpbug")
    d = os.path.join(work, "roms", "jumpbug")
    os.makedirs(d, exist_ok=True)
    files = {"jb1": program + bytes(4096 - len(program)), "l06_prom.bin": bytes(32)}
    for name in ["jb2", "jb3", "jb4", "jb5", "jb6"]:
        files[name] = bytes(4096)
    for name in ["jb7", "jbl", "jbm", "jbn", "jbi", "jbj", "jbk"]:
        files[name] = bytes(2048)
    for name, data in files.items():
        with open(os.path.join(d, name), "wb") as f:
            f.write(data)
    wav = os.path.join(work, "out.wav")
    env = dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
    subprocess.run(
        [os.environ["MAME"], "jumpbug", "-rompath", os.path.join(work, "roms"), "-video", "none",
         "-sound", "none", "-nothrottle", "-skip_gameinfo", "-seconds_to_run", "1",
         "-wavwrite", wav, "-samplerate", "192000"],
        cwd=work, env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    w = wave.open(wav)
    assert (w.getnchannels(), w.getsampwidth(), w.getframerate()) == (1, 2, 192000)
    n = w.getnframes()
    samples = list(struct.unpack(f"<{n}h", w.readframes(n)))
    os.remove(wav)

    # The calibration: seventeen plateaus of fixed amplitude 0-15 and 0 again, then the
    # envelope rising through its sixteen levels.
    r = runs(samples)
    fixed = [v for v, _ in r[:16]]
    assert len(set(fixed)) == 16 and r[16][0] == fixed[0], r[:20]
    envelope = [fixed[0]]
    for v, _ in r[16:]:
        if v != envelope[-1]:
            envelope.append(v)
        if len(envelope) == 16:
            break
    assert len(set(envelope)) == 16, envelope
    # Compare up to a little after the last change (the script ends quiet).
    last = max(i for i in range(1, n) if samples[i] != samples[i - 1])
    count = min(n, last + 2000)
    if os.environ.get("AY_KEEP"):
        with open(os.path.join(work, "mame.raw"), "wb") as f:
            f.write(struct.pack(f"<{count}h", *samples[:count]))
    samples = samples[:count]
    hashes = []
    for start in range(0, count, BLOCK):
        h = 0xCBF29CE484222325
        for v in samples[start:start + BLOCK]:
            for byte in struct.pack("<h", v):
                h = ((h ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
        hashes.append(h)
    with open(os.path.join(HERE, "jumpbug.mame"), "w") as f:
        f.write(
            "# MAME 0.289's AY-3-8910 on Jump Bug's board (1.536 MHz) playing jumpbug.bin, recorded\n"
            "# at the chip's own 192,000 samples a second, made by mame_ay.py: its output value for\n"
            "# each fixed amplitude 0-15 and each envelope level 0-15 (one channel playing, the\n"
            "# other two at 0), the samples compared, then an FNV-1a 64 hash of each 4096 of them\n"
            "# (as little-endian 16-bit values).\n"
        )
        f.write("fixed " + " ".join(map(str, fixed)) + "\n")
        f.write("envelope " + " ".join(map(str, envelope)) + "\n")
        f.write(f"samples {count}\n")
        for h in hashes:
            f.write(f"{h:016x}\n")
    print(f"{count} samples compared of {n}; fixed {fixed}; envelope {envelope}")


if __name__ == "__main__":
    main()
