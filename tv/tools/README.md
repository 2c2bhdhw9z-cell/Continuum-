# Tools for the TV version

All of them run in the build sandbox (x86-64 Linux), never on the owner's phone.

- `setup-toolchain.sh`: the 32-bit ARM (soft-float) Rust library, a glibc 2.31 sysroot and the
  linker wrapper, in `$TVB` (default `/projects/sandbox/.tvbuild`). Then `. $TVB/env.sh`.
- `../webos/package.sh <out>`: builds the TV's cBoy (`../cores/cboy`) and the app, and packs
  `dev.continuum.tv_<version>_arm.ipk`.
- `check-cboy.sh [frames]`: the TV's cBoy against the phone's on every test game. Every line
  must say SAME. Then the speed of both.
- `profile-cboy.sh <game> [frames]`: a sampling profile of the TV's cBoy, by function and line.
- `cboy-bench`: the program both scripts use (`HASH=1`, `PROF=1`, `BENCH_OPTS=...`); its `pcs`
  binary lists where the Advance's processor spends its instructions.

On the TV itself the app can measure a game: `continuum-tv --bench <core.so> <game> [frames]`
(pin it to a fast core with `taskset -c 3`), and a game played in the app writes its speed
("speed 57 fps, cBoy 17 ms a frame") to `/media/developer/continuum/log.txt` every 10 seconds.
