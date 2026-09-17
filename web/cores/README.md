# Core binaries

Emulator cores, fetched on demand by `src/engine/core-loader.js`. **Nothing here is
loaded at boot** — only `manifest.json` is, and that is metadata.

Binaries are *not* committed: they are build outputs from third-party GPL/LGPL
sources. Build them with `scripts/build-core.sh <name>`.

## What is real, and what is a placeholder

`manifest.json` marks each entry with a `kind`:

| kind | Meaning |
| --- | --- |
| `libretro` | A real core, built by `scripts/build-core.sh`. The JS runtime instantiates it as its own wasm module and hands Rust a handle. |
| `placeholder` | Bytes go to Rust, which substitutes its diagnostic pattern core. Geometry and timing in the manifest are still the system's real values, so the pacer and renderer are configured correctly. |

Today `fceumm` (NES), `mgba` (GBA, GB, GBC) and `genesis_plus_gx` (Mega Drive, Master
System) are `libretro`; the rest are placeholders awaiting the same treatment.

```bash
scripts/build-core.sh all              # all three real cores
scripts/build-core.sh fceumm           # → web/cores/fceumm.wasm          (1.7 MB)
scripts/build-core.sh mgba             # → web/cores/mgba.wasm            (1.5 MB)
scripts/build-core.sh genesis_plus_gx  # → web/cores/genesis_plus_gx.wasm (2.8 MB)
```

A system may be served by more than one core. `manifest.json` gives each entry a
`priority` (higher wins, real cores 100 and placeholders 10), the registry ranks the
candidates, and the user can override per system from the UI. That is why `gambatte`
and `smsplus` are declared even though they are still placeholders: they make `gb`,
`gbc` and `sms` one-to-many, which is what the subcore path needs in order to be
exercised at all.

## How a core is built

Not with Emscripten. Emscripten's libretro builds are whole-RetroArch bundles that own
the canvas, the audio graph and the main loop — the three things this architecture
keeps in Rust. Instead:

1. The core's C sources are compiled against **wasi-libc** (wasi-sdk clang) as a
   *reactor* module that exports the libretro C API.
2. `core-shim/libretro_wasm_shim.c` is compiled in alongside it. A libretro core calls
   the frontend through function *pointers*, and a host cannot manufacture a function
   pointer inside another wasm module — so the shim provides real in-module functions
   that forward to imports. It also flattens the two structs whose C layout a host
   would otherwise have to hard-code.
3. The result imports only `host.*` (six callbacks) and a dozen WASI file syscalls,
   which `core-runtime.js` stubs.

```
fceumm.wasm
  imports  host.{environment,video_refresh,audio_batch,input_poll,input_state}
           wasi_snapshot_preview1.{fd_*,path_*,proc_exit}
  exports  retro_* (17), shim_* (5), memory, malloc, free, _initialize
```

## Two build strategies

Libretro cores do not agree on a build system, so `build-core.sh` has two paths:

| Strategy | Used by | How |
| --- | --- | --- |
| `sources` | fceumm, Genesis Plus GX | The core ships a libretro makefile listing `SOURCES_C`; ask it for the list and compile those files directly. |
| `cmake` | mGBA | Configure with wasi-sdk's toolchain file and build the static library target. CMake also generates files the build needs — mGBA's `version.c`, for one. |

Both then link against `core-shim/` and export the same surface.

mGBA additionally needs wasi-libc's opt-in POSIX emulation (`_WASI_EMULATED_SIGNAL`,
`_WASI_EMULATED_MMAN`, process clocks, getpid) — `src/core/thread.c` includes
`signal.h` even with threading disabled — and it imports 19 WASI functions to fceumm's
12, adding clocks, `environ` and directory calls. Genesis Plus GX imports 14 and needs
wasm `setjmp`/`longjmp` (`-mllvm -wasm-enable-sjlj` plus `-lsetjmp`) for the Musashi
68000's address-error traps.

After linking, the script rejects any import outside `host.*` and
`wasi_snapshot_preview1.*`. A core that reaches for something else is then a build
failure rather than a blank screen at runtime.

## Adding another core

1. Add a case block to `scripts/build-core.sh` with the repository, strategy and flags.
2. Build it, then set the manifest entry's `module`, `sizeBytes` and
   `"kind": "libretro"`.
3. Nothing in Rust changes. `WasmCore` is core-agnostic, and the core's own
   `retro_get_system_av_info` supersedes whatever the manifest claimed.

Watch for three things that vary by core:

- **`need_fullpath`.** Cores that require a file path cannot run here. Most modern
  cores declare `need_fullpath = true` globally and then *override* it per extension
  (`SET_CONTENT_INFO_OVERRIDE` + `GET_GAME_INFO_EXT`), both of which the runtime
  implements. fceumm does exactly this.
- **Hardware rendering.** `SET_HW_RENDER` is refused: the renderer owns the GPU.
  Software-rendered cores only.
- **How the core decides which system it is.** Multi-system cores generally sniff the
  content's file extension, not its header. Genesis Plus GX reads the last three
  characters of `retro_game_info_ext.full_path`, so the launch path must pass a
  filename with the right extension — the ROM header alone is not enough.

`SESSION_HANDOFF.md` §1 lists every per-core compiler flag currently in use and the
symptom that made each one necessary. Worth reading before debugging a new core; several
of the failures are silent.

## Testing a core without a browser

```bash
python3 scripts/make-test-rom.py          # web/roms/nes-testcart.nes
./scripts/make-gba-rom.sh                 # web/roms/gba-testcart.gba
python3 scripts/make-sms-rom.py           # web/roms/sms-testcart.sms
node scripts/core-abi-test.mjs            # runs every built core headlessly
node scripts/core-abi-test.mjs mgba       # or just one
```

That harness loads a ROM, runs frames, and asserts on the actual pixels, audio,
input and save states — in under a second, with no GPU involved. It is the fast loop
for core work; the browser suite is the slow one.

Adding a core means adding a test ROM for it and an entry to the harness's `CORES`
table. Give it an idle colour no other cart uses: that is what lets a test say *which*
core drew a frame from the pixels alone, which is what makes the hot-swap checks in
`smoke-test.mjs` mean anything.
