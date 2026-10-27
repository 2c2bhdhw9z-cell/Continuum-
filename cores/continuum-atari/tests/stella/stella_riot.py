#!/usr/bin/env python3
"""Makes tests/references/timer-stella.txt: the RIOT timer checks (test-programs/timer_body.inc)
run on an Atari 2600, which has the same 6532 chip at the same addresses, in Stella, whose 6532 is
measured against real consoles. Stella runs as its libretro core (the app's pinned stella2023,
built for this machine), driven from here through ctypes; the readings are in the 2600's RAM.

    STELLA=.../stella2023_libretro.so python3 tests/stella/stella_riot.py
"""

import ctypes as C
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.normpath(os.path.join(HERE, "..", ".."))

ENV = C.CFUNCTYPE(C.c_bool, C.c_uint, C.c_void_p)
VIDEO = C.CFUNCTYPE(None, C.c_void_p, C.c_uint, C.c_uint, C.c_size_t)
AUDIO = C.CFUNCTYPE(None, C.c_int16, C.c_int16)
BATCH = C.CFUNCTYPE(C.c_size_t, C.c_void_p, C.c_size_t)
POLL = C.CFUNCTYPE(None)
STATE = C.CFUNCTYPE(C.c_int16, C.c_uint, C.c_uint, C.c_uint, C.c_uint)


class GameInfo(C.Structure):
    _fields_ = [("path", C.c_char_p), ("data", C.c_void_p), ("size", C.c_size_t), ("meta", C.c_char_p)]


def main():
    core = C.CDLL(os.environ["STELLA"])
    tmp = os.path.abspath("stella-dir").encode()
    os.makedirs(tmp, exist_ok=True)
    keep = []

    @ENV
    def env(cmd, data):
        if cmd in (9, 31):  # system and save directories
            C.cast(data, C.POINTER(C.c_char_p))[0] = tmp
            return True
        if cmd == 10:  # pixel format
            return True
        return False

    @VIDEO
    def video(data, w, h, pitch):
        pass

    @AUDIO
    def audio(l, r):
        pass

    @BATCH
    def batch(data, frames):
        return frames

    @POLL
    def poll():
        pass

    @STATE
    def state(port, device, index, id):
        return 0

    keep += [env, video, audio, batch, poll, state]
    core.retro_set_environment(env)
    core.retro_set_video_refresh(video)
    core.retro_set_audio_sample(audio)
    core.retro_set_audio_sample_batch(batch)
    core.retro_set_input_poll(poll)
    core.retro_set_input_state(state)
    core.retro_init()
    rom = open(os.path.join(CRATE, "test-programs", "timer2600.bin"), "rb").read()
    buf = C.create_string_buffer(rom, len(rom))
    info = GameInfo(b"timer2600.bin", C.cast(buf, C.c_void_p), len(rom), None)
    assert core.retro_load_game(C.byref(info)), "Stella loads the cartridge"
    for _ in range(10):
        core.retro_run()
    core.retro_get_memory_data.restype = C.c_void_p
    core.retro_get_memory_size.restype = C.c_size_t
    ptr = core.retro_get_memory_data(2)
    size = core.retro_get_memory_size(2)
    ram = C.string_at(ptr, size)
    values = ram[0:14]
    out = os.path.join(CRATE, "tests", "references", "timer-stella.txt")
    with open(out, "w") as f:
        f.write("# The RIOT timer checks (test-programs/timer_body.inc) on Stella's 2600 (stella2023\n")
        f.write("# ba52c43, the app's pin), made by tests/stella/stella_riot.py: values+0 to values+13.\n")
        f.write("values " + " ".join(f"{v:02x}" for v in values) + "\n")
    print("Stella:", " ".join(f"{v:02x}" for v in values))


if __name__ == "__main__":
    sys.exit(main())
