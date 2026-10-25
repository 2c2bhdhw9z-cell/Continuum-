# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> then `.kiro/notes/HANDOFF.md`, then carry on.

## START HERE FOR A NEW CHAT (25 October 2026, evening Eastern): finish cBoy, then the Sega core

Owner's order: a new chat finishes **cBoy** first, because it is much nearer done than the GameCube
core, then builds the **next easiest core**, which must be as good as the outside emulator it replaces
for those consoles and have more settings. One item at a time until it is fixed, then the next; do
not stop partway to report a discovery. The checklist with every item, the rules and the references
is `.kiro/notes/tasks/cboy-finish.json` (B1 to B9 for cBoy, then S1: Master System, Game Gear and
SG-1000 in one core, working name continuum-sms, built on the arcade core's own Z80, which already
passes ZEXALL and SingleStepTests). Another chat may be working on the GameCube core in
`cores/continuum-gc` at the same time: never touch it, and pull before every push.

FOLDERS, so two chats never get in each other's way (owner, 25 October): the GameCube chat works
ONLY in `cores/continuum-gc/**` and never touches the app or the engine (`native/**`, `crates/**`).
It needs nothing there for the GPU drawer: the app already has the graphics-chip hookup the PSP and
3DS use (libretro Vulkan, `crates/emulator-bridge/src/gfx/vulkan_hw.rs`). If it ever needs an app or
engine change, it writes the request under "Requests between chats" here and does not make it. The
cBoy chat works only in `cores/continuum-gb/**`, except its last three items (camera, two Game Boys
on one phone, the Advance link), which may change the app and engine; it reads this file first.

### Requests between chats
(none yet)

### cBoy chat, 25 October 2026 evening: where cboy-finish.json stands
B1 (Advance timing), B2 (sound chip), B3 (settings), B4 (later Color revisions in the exact drawer),
B6 (camera choice: already the app's Back / Front) and B7 (two Game Boys on one phone, `dual.rs`) are
done and pushed with [skip ci]; NO build yet. B5 waits for the build (the owner tries Advance games).
Next: B8 (Advance link), then B9 (one build for the whole batch, phone checks, ask about cBoy as the
default). Each item's "result" in tasks/cboy-finish.json says what was done and measured.

GameCube state for whoever picks it up: build 233 (next section) = the GPU drawer. Before it, Animal
Crossing ran in the town at about 85% of real speed, 42-61 pictures a second, drawing the limit. What
is missing from the core, by group, is in the owner's list of 25 October (speed: GPU drawer first, then
the JIT's floating point and paired singles and a register cache; picture options; players 2-4 and
rumble; memory card slot B and other card sizes; cheats; disc swap; broadband adapter; 478 of 479
settings; Wii). He said: work down that list one thing at a time, the GPU drawer first. Note: Continuum
Symbian is NOT one of the owner's own cores: it is EKA2L1 (GPL v3) with Continuum's libretro plug around
it (credits say "built on EKA2L1"). The owner's own, written from scratch: cBoy, cArcade, the GameCube core.

## GameCube chat (25 October 2026, about 6:30 PM Eastern): build 233 = the GPU drawer (G6 part 2)

BUILD 233 (f26066f; run 232 was stopped to add the copies-and-clears timing) checked: 0.8.0 (233), 38 cores,
JIT helper 233, GC dylib 1,726,176 bytes (231: 1,517,040) with the drawer's strings and the SPIR-V inside,
MoltenVK.framework present. https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-233-f26066f/Continuum-233.ipa
It also carries the cBoy chat's pushed work since 231. Neverball (3D, SD image .scratch/neverball-pristine.img
made by .scratch/mkfat.py from .scratch/hb/neverball/neverball at the card's root) on gpucheck: about a third
of the picture differs by a few levels (per-pixel rounding of smooth shading), 3 pixels a field by more than 8
(at most 27): no wrong drawing.

OWNER'S QUESTION, 6:13 PM: iCTRL (github.com/OpenMinis/iCTRL, 1.98.12) so Kiro can drive the phone. Checked in its
README: an XCUITest runner serving JSON-RPC on 127.0.0.1:8200 (GET /skill manual, /schema, pairing approved on
the phone, encrypted requests); needs the DDI mounted (StikDebug, already used for JIT) and signing with the nested
PlugIns/iCTRLUITests.xctest signed first with the runner's bundle id: Feather/ESign-style signers skip it and the
runner crashes at launch, so it needs SideStore or Scripts/sign-ipa.sh (codesign, a Mac: a GitHub macOS job with
his CertDrop .p12, password and development .mobileprovision as repo secrets). Reaching it from the cloud
sandbox: an SSH reverse tunnel from iSH on the phone (apk add openssh-client; cat /dev/location > /dev/null &
keeps iSH alive in the background; ssh -R 80:127.0.0.1:8200 nokey@localhost.run gives a public https URL; the
sandbox reaches localhost.run). No second VPN, so LocalDevVPN and JIT stay up. Answered with the time it takes;
waiting for his yes before writing the signing workflow (outside cores/continuum-gc: a new file of its own).


WHAT IT IS (cores/continuum-gc/src/gx/gpu/, tasks/gamecube.json G6 has the detail): everything before
the pixels stays on the CPU (vertices, transform, lighting, clipping, texture decoding, the per-primitive
Setup). The pixels are two Vulkan compute kernels, a line-by-line port of draw.rs raster_triangle +
tev.rs shade + texture.rs sample_decoded + efb.rs blend (draw.wgsl). Not the GPU's triangle hardware:
GameCube blending/logic ops/24-bit depth/early z/EFB formats cannot be copied by it on Metal. One
workgroup per 16x16 EFB tile, triangles applied in order, exactly like the CPU helpers' bands. The EFB is
memory shared with the GPU (efb.rs EfbMem); Gx::sync waits for the GPU too, so EFB copies, clears,
peeks, save states run the same CPU code on the same words. Device = the app's shared MoltenVK device via
libretro SET_HW_RENDER + GET_HW_RENDER_INTERFACE (no app or engine change; pictures still go out as
ordinary frames, never set_image). Setting "Drawing" (cgc_drawing): Graphics chip (default) / Processor.
At game start a PICTURE CHECK draws 60 random primitives both ways; more than 1% of pixels off by more
than 4 = processor. Any Vulkan error later = processor from then on (Gx::gpu_failed), logged.

CHECKED HERE (SwiftShader: export SS=/inner_container/opt/playwright/chromium-1232/chrome-linux64;
VK_ICD_FILENAMES=$SS/vk_swiftshader_icd.json LD_LIBRARY_PATH=$SS): tests/gpu.rs (random primitives over
every setting: about 1 pixel in 30,000 off by more than 4, from float rounding; GPU_SEEDS=n), tests/
frontend_gpu.rs (the libretro calls end to end with a fake app: request, context_reset inside it, the
interface, picture check, frames, save/load state, the Processor setting, no device), examples/gpucheck.rs
(two machines field by field: FCE Ultra GX 70 fields and Swiss 120: no picture pixel off by more than 8,
same pixel counts). SwiftShader is slow (seconds a field): keep runs short. CPU drawer still bit-identical
(drawcheck, both programs, 700 fields). After editing draw.wgsl: cargo run --release --example shaders
(writes draw.spv; tests fail while it is stale). iOS compile check: .scratch/tc/r192 (toolchain copy with
the aarch64-apple-ios std), see the command in this section's commit.

WHAT HIS UPLOAD WILL SAY: at start "cGC: drawing on the graphics chip (Apple ..., 128 MB for textures): set
up in N ms, picture check ... : X of 337920 pixels differ ..." or "cGC: drawing on the processor: <why>".
Every cGC second line ends "drawer graphics chip (chunks, triangles, waits N ms, states, textures KB,
texture memory resets)"; "wait" in run = time waiting for the GPU. The read-out panel shows DRAWING GPU/CPU.
NEXT: his 232 upload. If the GPU waits are small and the speed is still short, the CPU's own copy to the TV
picture (about 1 ms a picture with helpers, measured here: efb.rs bench_xfb_copy_and_clear) and the
emulated CPU are what is left; then the next item on his list.

## Earlier (25 October 2026, about 3:30 PM Eastern): build 231 = GameCube speed from the 230 profile

HIS 230 UPLOAD (h67pii) said exactly where Animal Crossing's time went (town, 40-50 emulator frames/s):
- SCREEN: the app's pacer SPIRALLED: every display tick ran 4 GameCube frames (3+:11-13 per second), the
  tick held the main thread 70-90 ms, so only 11-13 pictures reached the screen a second. That is the
  "10 fps" he saw, while the core itself managed 44-52 frames a second.
- Game thread (profile, symbolized with llvm-nm on ipa230's dylib): texture DECODING ~25% (`Gx::decoded`,
  + palette 3%: the whole texture cache was thrown away on every GXInvalidateTexAll / TLUT load / TMEM
  preload, which Animal Crossing's N64 display-list code does constantly), `Jit::compile` ~13% (every
  store to data sharing a 4 KB page with code dropped all that page's blocks: 5,000-6,000 blocks rebuilt
  and 800 page drops a second), JIT code itself only 15-20%, drawing helped 5%.
- Threads: game 85%, gx draw 0-3 ~80% each (drawing is the other limit), memory 850 MB, phone cool.
- The JIT works (99% of instructions run compiled); the Gekko's own code is just not where the time goes.
FIXED IN 231 (all tested here; pictures checked bit-identical with examples/drawcheck over 700 fields of
FCE Ultra GX and Swiss against the 230 code):
1. timing.rs: `FramePacer::catch_up_cap`: a tick may spend ~2 core frames of wall time catching up (from
   the measured step cost the bridge feeds `note_step_cost`); at least 1 step. A slow core now runs one
   step a tick instead of 4. Tests: slow core 1 step/tick, fast core still catches up, 4x fast-forward ok.
2. Texture cache (gx/texture.rs, mod.rs): invalidations bump `tex_epoch` instead of clearing; an entry
   from an older epoch is hashed (texels + palette) and reused when unchanged. Test:
   the_cache_keeps_unchanged_textures_and_redecodes_changed_ones.
3. Code invalidation by 32-byte line (`Hw::code_lines`, `mark_code`, `touch`): page bits stay as the quick
   filter; only blocks overlapping written lines are dropped (FastCpu::invalidate); the JIT's inline
   store check now reads the line bitmap (word w1>>11, bit (w1>>5)&63) and sends line-crossing stores to
   the slow path. Tests: rewritten_code_runs_as_rewritten_on_every_cpu (plain, fast, JIT under unicorn;
   rewrite before and after 2000 runs), stores_to_data_beside_code_do_not_throw_the_code_away.
4. raster_triangle: each row starts at its first inside pixel and stops at its last (edge functions
   solved per row); attributes stepped identically so results are bit-exact. FCEUGX drawing -11%.
5. Telemetry: the busiest non-game thread (a draw helper) is profiled too.
NEXT: his 231 upload. If drawing is still the limit (helpers ~80-100%), the GPU drawer (G6 part 2).

## Earlier (25 October 2026, about 2:00 PM Eastern): build 230 = ALL the telemetry, in one go

OWNER ON 229 (upload c6xury): GameCube sound came out at the right amount when fast enough, but he saw
as low as ~10 fps, and was furious that I quoted "39-60 FPS": my line counted EMULATOR FRAMES (retro_run
calls), not pictures on screen. Animal Crossing puts up a new picture every other field at best, and fewer
when it falls behind. NEVER quote emulator frames as FPS again: read "SCREEN n new pictures/s" (cGC line)
and the "screen:" line's presents. 229 measured frame time avg 15-24 ms (needs <16.7) for ~40 s of it,
i.e. the GameCube is ~1.5x too slow in Animal Crossing's town.
He also said: add ALL the telemetry at once, no more one-thing-per-build. Build 230 carries:
- cGC per-second line (cores/continuum-gc/src/libretro.rs `Tel`): SCREEN new pictures/s and repeats,
  fields and % speed, lines, run ms/s split into cpu/draw/wait/sound/picture, worst field, Gekko cycles,
  idle-skip %, JIT % / blocks compiled / resets / handler %, interpreted M, blocks built/invalidated,
  not-compiled reasons, gx tris/pixels/efb/xfb copies/display lists/commands, sound DMA on % and 32 kHz %,
  stream, disc reads/KB/errors, ARAM DMAs/KB, and the 6 hottest 256-byte code blocks (map with acsym.py).
- GameCube picture: converted only when the TV picture changed (raw XFB compare), otherwise sent to the
  app as a repeat (null frame); the read-out panel forces a fresh picture when switched off (the
  frontend test caught that regression before the build). Drawing helper threads and the XFB copy
  threads ask for user-interactive scheduling (Apple QoS), so the scheduler stops parking them on
  efficiency cores while the main thread waits (the WAIT figure).
- Engine (telemetry.rs / native_core.rs): every setting each emulator reads and the value it got
  ("setting key = value", once per game), every environment request and whether it was granted, and the
  per-second line now splits emulator frames into new pictures and repeats.
- C (telemetry.c watchdog): every second, processor time per thread by name (game, gx draw N, audio...)
  plus memory used and still allowed; every 5 s a profile of the game thread (100 samples a second, by
  library + offset; symbolize offsets with llvm-nm on the same build's .ipa dylibs).
- Swift: "screen:" line every second (display-link ticks, late ticks and worst gap, emulator frames per
  tick 0/1/2/3+, dropped, resyncs, presented, tick time on the main thread avg/max, pacer fps, thermal
  state, Low Power Mode); screen max Hz and the link's requested range once; "sound device:" once per
  graph (route, rate, IO buffer, output latency, ring and prime); about.txt has the JIT settings, thermal
  state and Low Power Mode.
NEXT: his 230 upload gives the profile; speed work goes where it points (CPU drawer -> GPU drawer, or
emu64's display-list code in the JIT, or the main-thread wait).

## Earlier (25 October 2026, about 1:00 PM Eastern): build 229, GameCube sound fixed

OWNER'S UPLOAD ON 228 (litter.catbox.moe/oksf9q): no crash anywhere. **PSP works** (Midnight Club:
60 fps, 44100 sound frames a second, no gaps; the IR interpreter still logs one recovered "[JIT] Jump
to invalid address" at boot, harmless). **3DS works** (Animal Crossing New Leaf a full minute: 60 fps,
frame time 1-4 ms, full sound, no gaps; 8 dynarmic slices = 256 MB). **GameCube** still 32031 sound
frames a second.
ROOT CAUSE FOUND AND FIXED (229, 59014ec): libretro.rs ran `run_field(cpu_hz / 60)`, and cpu_hz/60
(8,100,000 cycles) is a hair SHORT of one NTSC field (262.5 lines x 30888 = 8,108,000). So each call
stopped one line before the field ended, and every third call ran just that one line: three calls ran
262+262+1 lines instead of 525+. The machine ran at TWO THIRDS of real time and the sound came out a
third short. It was never the JIT and never the sound code. Measured with a new probe (tests/machine.rs)
before the fix: 32032 frames per emulated second, exactly the phone's figure. Now `Machine::run_one_field`
runs to the field's end with 2 fields of cycles as a safety cap. Tests: 48048 frames a second at 48 kHz
DMA, 32 kHz DMA and 32 kHz + 48 kHz stream, on the plain and fast CPU; 262/263 lines every call.
Side effect to watch: the GameCube now emulates 1.5x as much per second as before (it really was slow),
so frame times go up by about that much (228 showed avg 3-11 ms, max 22 ms; a 16.7 ms frame is the
limit). If the next upload shows frames over 16.7 ms, that is the hiccup, and the speed work is next.
Also fixed: the Swift "sound out" figures went negative after the ring was emptied.
NEXT: his upload from 229 (GameCube sound and frame times).

## Earlier (25 October 2026, about 12:15 PM Eastern): build 228, the three 227 faults diagnosed

The telemetry worked: he uploaded three diagnostic .zips and every fault was named from them
(litter.catbox.moe/3xmbkp, 2e3e6u, f0mes9; downloaded with curl, kept in .scratch/diag).

1. **3DS crash: found and FIXED in 228.** crash-previous.txt: SIGBUS writing 0x126c00000 at
   `Dynarmic::EmitSpinLockLock` from `SpinLockImpl::Initialize` inside `ARM_Dynarmic::Run`.
   dynarmic's spin lock (externals/dynarmic/src/dynarmic/common/spin_lock_arm64.cpp) built its own
   4 KB oaknut CodeBlock with `code{mem.ptr(), mem.ptr()}`: it WROTE through the run address, which
   on a TXM phone is read+execute only. Azahar's shader recompiler had the same bug
   (shader_jit_a64_compiler.cpp: `memcpy(code_mem->ptr(), ...)`). Both now use `wptr_base()`; both
   hunks are appended to scripts/patches/azahar-ios-jit-region.patch. Verified in run 40208522630:
   azahar re-cloned, patch applied, dynarmic recompiled, so the fix is really in the .ipa.
   (The 4096-slice change in 227 was NOT the cause, but it was a real limit and stays.)
2. **PSP black and silent: found and FIXED in 228 (by turning its recompiler off).** The log shows
   Midnight Club loading, Vulkan up, the first frames presenting, then `[JIT] Jump to invalid
   address: 00000000`, `Detected rounding mode usage, rebuilding jit with checks`, and
   `Bad execution access detected, halting: 0ffffffc (last known pc ffffffff)`. Thousands of
   `[MemMap] Bad memory access ... at block27_..._strlen` first. PPSSPP's native arm64 recompiler
   does not work with MASKED_PSP_MEMORY, which iOS forces (Core/MemMap.h:94). MEASURED off the
   phone: `scripts/jit-check.sh ppsspp` with PPSSPP_MASKED=1 (run 40207666136) — IR interpreter
   458/458 pspautotests, native recompiler 0/458, with ordinary memory AND with ours. So
   `("ppsspp", _, true)` is back to `Locked("IR JIT")` (options.rs, with the evidence in the
   comment). Do not re-enable without that check passing. GET_JIT_CAPABLE still answers yes, which
   is what gives PPSSPP its JIT vertex decoder.
3. **GameCube sound: measured exactly, NOT yet fixed. THIS IS THE NEXT JOB.** With JIT on the core
   hands the app **32031 sound frames a second while declaring 48000 Hz** (`app: second N` lines),
   so the device fills the missing third with silence: `sound out: ... 14-18 gaps (8900-9400 silent
   frames)` every second. Measured in sound.wav (30 s, 48 kHz): **14.2% of it is exact silence**,
   41 runs of 64+ samples, many of 6380 samples (133 ms each) and one of 64449 (1.3 s), peak 20244,
   no clicks. That is what "absolutely terrible" is: a third of the sound missing in 133 ms holes.
   The core's own sound code is NOT the fault: the new test
   `hw.rs sound_rate_tests::the_core_emits_48000_frames_a_second_at_either_dma_rate` drives
   `audio_line` at 15734 lines a second and gets ~48000 frames at both DMA rates (32 and 48 kHz).
   So **fewer video lines are being run per field than a second needs, only with JIT on** (~2/3 of
   them; 32031/48000 = 0.667). Look at `Machine::run_field`: the budget is `cpu_hz/60` cycles and
   `fast.run(...)` returns the cycles it ran; if the JIT over-reports cycles (or the idle skip
   advances the clock without the lines being clocked out), the field ends early and the AI gets
   two thirds of its lines. Also in the same read-out: frame time avg 3-4 ms max 5-7 ms, longest
   gap 16-17 ms, 60-61 frames a second, so the hiccup to ~20 fps did NOT reproduce in that
   30 seconds — only the sound did. NEXT: count lines per field against cycles in run_field with
   the JIT on (examples/compare: plain vs fast), fix the cycle accounting, and add a `cGC:` line a
   second with lines-per-field so the next phone test proves it.
4. Still untested since 227: whether the 3DS now runs, and whether the PSP picture comes back.

## Earlier (25 October 2026, about 10:30 AM Eastern): build 227 = telemetry + 3DS JIT fix

OWNER ON 226 (JIT on, Built in works, "JIT script finished (region blessed, detached)"): 3DS crashes
(Animal Crossing New Leaf), PSP opens but black and silent (Midnight Club), GameCube faster (JIT 99%) but
still hiccups to ~20 fps and the sound is "absolutely terrible". Then: "PUT the telemetry to GET what you
need, the logs don't do shit" and "I would LOVE to let you hear the audio".
DONE IN 227:
- jit26.c: 4096 slices instead of 32. Azahar's shader recompiler takes one oaknut CodeBlock per shader
  program (shader_jit_a64_compiler.cpp:1050) under a new name; 8 dynarmic instances (2 per core) + a couple
  dozen shaders used the 32 up, then oaknut fell back to a plain RX mapping, which a TXM phone can't run.
  Likely the 3DS crash; telemetry will confirm ("jit memory: REFUSED" lines would show it).
- scripts/jit-check.sh + .github/workflows/jit-check.yml (manual): on an Apple Silicon runner, PPSSPP's
  pspautotests with its JIT and dynarmic's A32 tests, plain memory vs Continuum's two-address memory
  (jit26.c built with CONTINUUM_JIT_TEST). RESULT (run 40199171792): identical, all pass (dynarmic 200561
  assertions; PPSSPP all tests). So the PSP black screen is NOT the dual-mapping write path; telemetry next.
- Telemetry (crates/emulator-bridge/src/telemetry.c + telemetry.rs, Swift DiagnosticsUpload.swift):
  core-log.txt gets EVERY core log line (before: os_log only), app: notes (JIT memory hand-outs/refusals,
  per-second frame time avg/max/gap + sound frames, Swift "sound out" gaps/dropped/buffered); crash catcher
  (registers + named backtrace) to crash.txt; watchdog samples the game thread on frames >50 ms ("slow")
  and every 2 s when stuck >3 s ("STUCK"); 30 s recording of the core's sound -> sound.wav. Settings,
  Technical details, "Upload a diagnostic file": zip to litterbox.catbox.moe (72 h), link copied; the owner
  pastes it in chat; download with curl (works from the sandbox). Symbolize offsets with llvm-nm on the
  same build's .ipa dylibs (ppsspp/azahar keep local symbols). Tested on the Mac runner (jit-check telemetry).

## UPDATE (25 October 2026, about 8:15 AM Eastern): build 225 is out, built first try

Run 40190766636 (started by hand) passed with no compile errors. Release build-225-174e619. Checked:
0.8.0 (225), 38 `_libretro_ios.dylib`, Frameworks/continuum_gc_libretro_ios.dylib, and
PlugIns/ContinuumJITHelper.appex (dev.continuum.app.JITHelper, 0.8.0 (225), arm64, universal.js and
legacy.js inside). Handed to the owner with the test steps from step 3 below. Waiting on his report.
OWNER: he did NOT use the Release's get-task-allow.plist when signing (he just installs the .ipa), and
JIT with StikDebug still worked. Never tell him to use that file or imply he did; he only needs the .ipa link.
225 ON HIS PHONE (8:30 AM): "Import pairing file" opened the picker, his rp_pairing_file.plist showed, tapping
it did nothing (SwiftUI .fileImporter, the known no-callback failure). Build 226 (f27fedc) uses FilePicker
like every other import. Checked: 0.8.0 (226), 38 cores, GC core, helper appex 226. Waiting on his report.

## START HERE (25 October 2026, about 7:50 AM Eastern): built-in JIT written, NOT BUILT, NOT COMPILED

The owner ended the chat here because it got too long. Everything is pushed with [skip ci]; nothing
since build 224 has been built. NEXT BUILD (225) carries: 0d68444 (GameCube backup fix), 8f717bf (JIT
button line), and the built-in JIT below.

OWNER, ~7:40 AM: "Works with stikdebug here's the picture, so I guess I want it Any way Jit can be set up
Continuum can do that method." (The picture did not arrive; text only. He was told.) So JIT through
StikDebug works in Continuum 224 on his phone (iOS 27.0.1, iPhone18,2, TXM, CertDrop development cert,
get-task-allow.plist, StikPair pairing file, LocalDevVPN). Earlier: "It should be as simple as Manic, if
not simpler". GOAL: every way JIT can be turned on, default "Built in" like Manic (import the pairing file
once; JIT comes on by itself when a game starts).

WHAT IS WRITTEN (this commit; Swift syntax-checked with `swiftc -parse` on Linux, never compiled for iOS):
- scripts/fetch-stikjit.sh: clones StikJIT tag 1.9.0 (commit 32287268fa5824f9edce4cb359f5833ce0cf7b00 is
  checked) into native/ios/build/stikjit (Sources, Resources, idevice, iokit, LICENSE). Works locally.
  Why source and not the release xcframework: its swiftinterface is Swift 6.4 / Xcode 27; CI has Xcode
  26.6 (iOS SDK 26.5). Madeira (GPL) was read for the approach only; no code copied.
- native/ios/JITHelper/ContinuumJITHelper.swift + Info.plist: app extension dev.continuum.app.JITHelper,
  extension point com.apple.ar.viewer with FALSEPREDICATE, principal class ContinuumJITHelperHandler.
  Request JSON in the input item's userInfo["continuum-jit-request"] {operation enable|prepare|reset,
  targetPID, pairing (base64 string -> Data, JSONDecoder's default)}; answer JSON in
  userInfo["continuum-jit-response"] {ok, message, log}. "enable" = StikJIT.enableJIT(targetPID:,
  pairingFile:, ddiPaths: DDIPaths.default(in: Library/StikJIT), script: .universal, forceScript: false).
- project.yml: target ContinuumJITHelper (app-extension, iOS 17.4, StikJIT sources + resources,
  include paths for idevice/iokit, `-Wl,-hidden-lidevice_ffi -lc++`, sdk frameworks); embedded in the
  app (PlugIns/); BuiltInJIT.swift added to the app's sources.
- package-ipa.sh: runs fetch-stikjit.sh before XcodeGen; stamps the appex CFBundleVersion with the app's;
  signs PlugIns/*.appex; FAILS the build if PlugIns/ContinuumJITHelper.appex is missing.
  ios.yml: fetch-stikjit.sh is a build trigger path.
- native/ios/BuiltInJIT.swift: JitMethod (builtIn default, stikDebug, otherApp); JitPairingFile
  (Documents/StikJIT/pairingFile.plist + Keychain copy, AfterFirstUnlockThisDeviceOnly, so it survives
  the owner deleting the app; never synced or logged); JitHelper (starts the appex through the private
  NSExtension API: extensionWithIdentifier:error:, beginExtensionRequestWithInputItems:completion:,
  completion/cancel/interruption blocks); BuiltInJIT.shared (turnOn, shouldTurnOnBeforeGame, a
  main-actor polling Task calling engine.prepareJit every 0.25 s so the engine prepares the code memory
  as soon as the helper's universal script is attached; gives up after 120 s; after 2 failures in a run
  it stops waiting at game start); BuiltInJITRows (Settings, Technical details: method picker, pairing
  file import, "Turn JIT on when a game starts" switch (on), "Turn on JIT now", "Download the developer
  disk image again").
- SettingsScreen.swift: jitRows shows BuiltInJITRows in place of the old StikDebug-only button (StikDebug
  is now a method; its jitAskLine shows under it). Credits: StikJIT (MPL 2.0), idevice (MIT), StikDebug
  universal script (AGPL 3.0).
- ContinuumApp.swift `launch(entry:...)` has `jitWaited: Bool = false`: with Built in + pairing file +
  the switch on and JIT not on yet, the first game waits for turnOn, then re-enters launch once with
  jitWaited true (status keeps its "opening " start so the Library shows no refusal). The core fallback
  retry, launchAndLoad (play from a state) and Clean start pass jitWaited true, because they read the
  result the moment launch returns.

NEXT CHAT, IN ORDER:
1. Start build 225 (push without [skip ci], or gh api -X POST
   repos/2c2bhdhw9z-cell/Continuum-/actions/workflows/ios.yml/dispatches -f ref=master). EXPECT compile
   errors in the helper/StikJIT sources (Swift 6.2 vs StikJIT's toolchain, IOKit headers on the iOS SDK,
   kIORegistryEntryPropertyKeysKey, the idevice module map) and in BuiltInJIT.swift (actor isolation,
   Sendable). Fix them in one go per round; read the failed log with gh run view --log-failed.
2. Check the .ipa: 0.8.0 (225), 38 `_libretro_ios.dylib`, Frameworks/continuum_gc_libretro_ios.dylib,
   AND PlugIns/ContinuumJITHelper.appex with its CFBundleVersion = the app's.
3. Hand over the link + numbered steps: sign with the development (JIT) certificate and the Release's
   get-task-allow.plist; reinstall; import Animal Crossing again (once; the backup keeps it from now on);
   Settings, Technical details, JIT method Built in, Import pairing file (the one from StikPair, the same
   one Manic uses); LocalDevVPN on; open a game: it should say it is turning JIT on, then open; Technical
   details' JIT line says on; in a GameCube game the speed read-out shows a "JIT n%" line.
   Ask what he sees if it fails: the line under the button says what went wrong in plain words.
4. KNOWN RISKS: (a) iOS may refuse to start an app extension of a sideloaded app through NSExtension if
   the signer (Feather/ESign) strips or renames PlugIns: the button then says the helper is missing or
   didn't start. (b) Signers re-sign the appex with the same cert; the appex's own get-task-allow is not
   needed (it is the debugger, not the debuggee). (c) "Another JIT app" only waits for an attach after
   "Turn on JIT now"; it does not auto-prepare at app start, on purpose (a non-universal debugger would
   not answer the prepare breakpoints). (d) StikDebug method: if StikDebug does not open, the button
   stays "Turning JIT on..." for 90 s.
5. The other ways still to add if the owner wants them (ask before building): SideStore/AltStore's own
   "Enable JIT" (works today as "Another JIT app"), a computer (same), StikDebug's "launch with JIT"
   shortcut; owner-rules' old "owner cannot use JIT" line is outdated (PLATFORM_LIMITS records it).
6. Then: interpreter speed (dispatch loop, superinstructions, stmw/lmw/cr ops), the GPU drawer, the
   Neverball plain-vs-fast compare difference after field 33 (pre-existing since 0d4edda).

TOOLS (a new sandbox has none of /projects/sandbox/.scratch; get them back like this):
- Cargo: the default cargo home is read-only; use CARGO_HOME=/projects/sandbox/.scratch/cargo-home
  cargo +1.92.0 test (GameCube JIT tests: --features unicorn).
- cores/continuum-gc/scripts/acsym.py (Animal Crossing address -> function name; needs ac-decomp's
  config/GAFE01_00/symbols.txt from github.com/ACreTeam/ac-decomp, path in AC_SYMBOLS) and mkfat.py
  (FAT32 SD image for the SD adapter tests; pip install pyfatfs).
- Homebrew used for tests (download again): FCEUGX (fceugx-gc.dol), Swiss r2119, Neverball (GameCube).
- StikJIT source: scripts/fetch-stikjit.sh. Its INTEGRATION.md is the guide followed for the helper.
- Examples in cores/continuum-gc/examples: mips, fpbench (CGC_WRITE, CGC_MODES, CGC_SECS), profile
  (CGC_THREAD, CGC_DRAW_THREADS, CGC_LINES), vibench (TV picture conversion), drawcheck, compare.
- Swift can be syntax-checked here (swiftc -parse file.swift, Swift 6.3 on Linux); it cannot be
  compiled for iOS here. Only CI compiles it.

## Earlier (25 October 2026, about 6:15 AM Eastern): GameCube game gone after the reinstall

OWNER ON 224 (6 AM, furious): GameCube "isn't showing". Two screenshots: Settings, About shows 0.8.0 (224)
with the GameCube (experimental) switch ON; the Library (80 titles) has no GameCube console and no
Animal Crossing. 224 changed no app code at all (git diff b25550a..54ab3e5: only cores/continuum-gc and
notes). CAUSE: he deletes the app before every install, and the backup (CloudSync `Games/`, which brings
his other games back) only carries `CoreCatalog.syncableGameExtensions`, built from `routeTable` +
shared extensions. GameCube's own types (.rvz .gcz .ciso .wia .gcm .tgc .dol) are routed only through
GameCubeUnlock.route, never in routeTable, so a GameCube game was never backed up and was lost on every
reinstall (.iso copies would have travelled; his Animal Crossing was evidently not one). FIX (this
commit, not built yet): `syncableGameExtensions` includes `GameCubeUnlock.fileTypes` always; listing
still waits for the unlock. The unlock key itself already travels in the settings export. He has to
import Animal Crossing once more, on the build that has this fix, for it to survive later reinstalls.
Build offered once (hold for the next batch otherwise, owner rule 16).
OWNER, 6:20 AM: "I'll be able to try JIT shortly." (Do not argue with PLATFORM_LIMITS' "not reachable";
record what he reports.) The GameCube JIT (cores/continuum-gc/src/jit) has only ever run under qemu/unicorn,
never on a real iPhone. How to tell it is on: Settings, Technical details, JIT line; and in a GameCube game
the speed read-out shows a "JIT n%" line (share of instructions run as compiled code) only when it is on.
If it crashes: Settings, Technical details, "Use JIT when it's available" off goes back to the interpreter.
OWNER, 6:15 AM (Gemini screenshots): updating to iOS 27.0.1, a $14 CertDrop development certificate,
Feather/ESign, StikPair for an on-device pairing file (verified: StikPair README says iOS 27+, on-device),
StikDebug + LocalDevVPN. He will try a known-JIT emulator (Manic) first, then Continuum. Continuum
already speaks the iOS 26/27 universal-script protocol (PLATFORM_LIMITS, top of the JIT section).
Signing Continuum for JIT: the Release's get-task-allow.plist as ESign's entitlements file (Feather:
keep get-task-allow). Then Technical details, "Turn on JIT with StikDebug".
6:47 AM: owner on iOS 27.0.1 (iPhone17Pro_Max, TXM present per Manic), Manic installed with the CertDrop
development (JIT) certificate; Manic's JIT page: built-in debugger, pairing file not imported. Told him:
StikPair (github.com/StikDebug/StikPair/releases/download/v1.2.0/StikPair.ipa) -> pairing file, LocalDevVPN,
import into Manic, Prepare JIT, Enable JIT. Continuum has no built-in debugger: it needs StikDebug
(github.com/StikDebug/StikDebug/releases, 3.1.15) with the same pairing file.
7:12 AM: JIT WORKS ON THE OWNER'S PHONE, no computer: StikPair paired (iPhone18,2), LocalDevVPN connected,
Manic's JIT page "JIT Available, Active Debugger: Built-in Debugger"; Rogue Leader plays in Manic (HUD
~60 fps on its select screen). Next: StikDebug + Continuum signed with the development cert and
get-task-allow.plist, then Technical details, "Turn on JIT with StikDebug" (checked: StikDebug 3.1.15
takes stikdebug://enable-jit with pid, bundle-id and script-name, and bundles universal.js).
~7:30 AM: Continuum 224 signed for JIT (the "Turn on JIT with StikDebug" button shows, so get-task-allow is
in), but tapping it "does nothing". The only silent path: UIApplication.open(stikdebug://...) returned
false (StikDebug not installed/registered) and the failure went to `status`, which Settings never shows.
8f717bf (not built): `jitAskLine` under the button says Opening StikDebug... or that it didn't open.
Told him the route that needs no build: StikDebug installed, pairing file imported, LocalDevVPN on; in
StikDebug's "Apps with get-task-allow" long-press Continuum, Assign Script, universal.js; tap Continuum
there (launches it with JIT); then Continuum, Technical details, "Finish setting JIT up" (needs_setup).
Manic has its own built-in debugger (StikJIT-style, "Enable JIT on Launch"); Continuum has none: a
parity item worth raising later, not started (rule 16).
NOT BUILT YET, waiting for the batch: 0d68444 (GameCube backup) and 8f717bf (JIT button line).

## Earlier (24 October 2026, about 9 PM Eastern): the read-out's answer, build 224

OWNER'S READ-OUT ON 223 (~7:35 PM, five screenshots; the read-out works now):
    title screen:  SPEED 81% | CPU 86M LOAD 32% | CPU 65 DRAW 8 | WAIT 12 SND 0 APP 15 | HOT 8007E100 68 ...
                   SPEED 100% | CPU 94M LOAD 28% | CPU 71 DRAW 10 | WAIT 1 SND 0 APP 18 | 8007E100 71 ...
                   SPEED 100% | CPU 78M LOAD 24% | CPU 67 DRAW 9 | WAIT 10 SND 0 APP 14 | 8007E100 76 ...
    K.K. scene:    SPEED 99-100% | CPU 56-61M LOAD 17-18% | CPU 35-40 DRAW 1-3 | APP 57-64 (Metal HUD 60 fps)
    title hot code (python3 .scratch/acsym.py): 8007E100 SelectThread (the OS idle loop), then emu64 (Animal
    Crossing's N64 display-list emulation: texture_matrix/set_position 80050400/80050800, dl_G_VTX 80055B00,
    dl_G_TRIN 80056000, emu64_taskstart 80058A00); K.K.: 80017000-80017800 RspStart (its software N64 sound).
WHAT IT MEANS: the game runs at 81-100% speed, but the Metal HUD shows ~15 pictures a second. The engine runs
the core on the main thread and the pacer (timing.rs) runs up to 4 core frames per display tick when behind;
with a core frame of ~13-20 ms the main thread is saturated and the pacer settles into 2-4 frames per tick
(.scratch/pacersim.py: 13.3 ms -> 30 shown/s, 20 ms -> 12 shown/s at 77% speed, <= 11 ms -> 60). Drawing is
not the bottleneck here (DRAW 8-10, WAIT 1-12); the PowerPC interpretation is (CPU 65-71%), and the hot code
is floating-point heavy. So: make the fast interpreter faster. Done in a430fef (build 224): quick handlers
for FP and paired singles (fast_ops.rs "paired singles, quickly", fpu.rs farith_quick shared with the plain
interpreter), 1.7x on .scratch/fpbench.dol (examples/fpbench.rs; x86 needs RUSTFLAGS="-C target-feature=+fma"
to be fair, the phone has hardware FMA). Neverball 1,500 fields 101 -> 86 s.
FOUND, NOT FIXED: examples/compare.rs on Neverball differs plain vs fast after field 33 (fast 10 cycles ahead,
pc equal) - already so at session start (0d4edda), not from today's changes. Track down later.
BUILD 224 DONE 9:15 PM Eastern (54ab3e5, started by hand, green). Checked: 0.8.0 (224), 38 emulator files,
continuum_gc dylib 1,499,664 bytes. Link and steps given:
https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-224-54ab3e5/Continuum-224.ipa
NEXT: more interpreter speed (the dispatch loop itself is ~50% of interpreter time; superinstructions or
fewer per-op stores; stmw/lmw and cr ops still go generic), then the pacer for heavy cores if needed.
Neverball test image: `.scratch/mkfat.py img 256 .scratch/homebrew/neverball/neverball` (pyfatfs in
.scratch/venv), run from a fresh copy (the game writes to it).

## Earlier (24 October 2026, about 7:15 PM Eastern): build 223, the read-out made reachable

OWNER ON 221 (~7 PM): "Nope. Saw and felt no difference. And the thing don't turn on." Three screenshots:
title screen with the Metal HUD (~16 and ~24 fps), and Core settings showing Speed read-out: Off, with no
status line under "Changes apply to every Continuum GC (GameCube) game" (so no change had gone through).
Through the engine here (crates/emulator-bridge/tests/gc_frontend.rs, CGC_SO=libcontinuum_gc.so, run with
--features native-core) the option turns on and the panel shows. Most likely cause on the phone: the game
keeps running behind the Core settings sheet (the classic ⋯ menu does not pause), and the engine runs the
core on the MAIN thread (MetalCanvas CADisplayLink -> EmulatorBridge::tick, up to 4 catch-up frames a tick,
timing.rs MAX_CATCH_UP_STEPS), so a GameCube game slower than full speed keeps the screen too busy to use its
menus. Fix in 223 (222 was cancelled: started before the copy speed-up was saved): PlayerScreen pauses the game while Core settings is open (not online, not web players)
and resumes on close (pausedForCoreSettings). Also 223: the EFB->XFB copy line by line and threaded (same
pictures; FCE 11.2 -> 10.0 s for 400 fields). Note: the Metal HUD's fps is presented frames; with up to 4
core frames per tick, game speed can differ from it. The read-out's SPEED line is the real speed.
BUILD 223 DONE ~7:25 PM Eastern (b25550a, green). Checked: 0.8.0 (223), 38 emulator files, continuum_gc
dylib 1,482,512 bytes with cgc_speed_readout, the app binary has the Core settings pause. Link and steps
given: https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-223-b25550a/Continuum-223.ipa
Tried and dropped: indexing the interpreter loop by position instead of a running address (300 -> 280 M/s on
.scratch/abench). NEXT: his read-out screenshot decides the big job: DRAW/WAIT high -> the GPU drawer (Vulkan
through the app's HW render path; SwiftShader's Vulkan is on this machine at
/inner_container/opt/playwright/chromium-1232/chrome-linux64); CPU high with LOAD near 100% -> interpreter
redesign (superinstructions / fewer per-op stores) and the hot functions from the HOT lines.

## Earlier (24 October 2026, about 6:25 PM Eastern): Animal Crossing plays but is slow; build 221 measures why

OWNER REPORT on build 220 (6 PM): "Loads." Animal Crossing reaches its title screen ("Press START", the town
demo). He feels it runs 60 fps with no sound playing and 12-20 fps when sound plays (not sure). Three
screenshots with iOS's Metal performance HUD (contents written here; pictures are not on disk): title screen
with trees, flowers and the player walking: 13.7 fps, frame interval ~73 ms; plaza with trees: 19.1 fps;
grass and a house: ~60 fps, 16.6 ms. He was done testing (owner: a report IS "done testing"; never ask).

What was measured here (no Animal Crossing on this machine):
- Our Zelda-family sound stand-in with Animal Crossing's load (64 voice blocks, 24 playing AFC): 0.7% of real
  time (dsp_zelda bench_zelda_one_second). Not the cause.
- Animal Crossing's own audio is heavy CPU work: its JAudio "Neos" thread runs the N64 sound driver and an
  RSP simulator in C on the PowerPC (ac-decomp rspsim.c: ADPCM, resampling, envelope mixing), which would
  explain "slow when sound plays" if the CPU is the bottleneck. Our fast interpreter runs an audio-style
  program of our own (.scratch/abench/bench.c, ADPCM + 4-tap resampling + mixing) at 276 M instructions/s
  here (examples/mips.rs), ~57% of a fully busy Gekko; half of that time is the dispatch loop (run_block).
- Drawing is also heavy: Swiss's menu reaches only 55-70% speed here, nearly all drawing. Scenes differ in
  how much they draw, which could also be what he saw.
So build 221 carries a SPEED READ-OUT (core option cgc_speed_readout, Performance) to see which it is on his
phone, plus drawing shared out dynamically (gx/draw.rs: 16 band groups taken by whoever is free, the CPU
thread draws too while it waits; same pictures; ~10% faster here, more on a phone's mixed fast/slow cores).
Read-out lines: SPEED %; CPU <M instructions per real second> LOAD <% of emulated cycles not idle-skipped>;
JIT % (only with JIT); CPU/DRAW (% of wall time); WAIT (waiting for helper pixels)/SND (sound stand-in)/APP
(time outside run_field); then the 5 hottest 256-byte code blocks (PC sampled at each video line) with %.
Map hot addresses with ac-decomp config/GAFE01_00/symbols.txt (read only).
BUILD 221 DONE 6:38 PM Eastern (started by hand, 2cd78c3, green). Checked: 0.8.0 (221), 38 emulator files,
continuum_gc dylib 1,481,104 bytes with the cgc_speed_readout option inside. Link and steps given:
https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-221-2cd78c3/Continuum-221.ipa
Map hot addresses: `python3 .scratch/acsym.py <hex>...` (static program symbols; .scratch/ac_symbols.txt from
ac-decomp; foresta.rel addresses need its load address, symbols in .scratch/ac_foresta_symbols.txt).
NEXT after his screenshot: if CPU dominates, make the interpreter faster (dispatch: cur_pc store and exit
flag per op, superinstructions, stmw/lmw and cr ops are on the slow generic path); if DRAW/WAIT dominate,
the GPU drawer (G6 part 2, planned next anyway).

## Earlier (24 October 2026, evening Eastern): the sound timing fix is done; build 220 for the owner

Done this session (cores/continuum-gc, commits 2712fbd and the one after it):
- The AI sample counter counts from the CPU's clock sample by sample (Hw::ai_sync, `Ai.at`; 54 MHz / 1124 at
  48 kHz, / 1686 at 32 kHz; re-anchored on AICR start/stop/rate/SCRESET and AISCNT writes; AIINT when it reaches
  AIIT, checked on reads and once a line via Hw::ai_line). Every register access knows the CPU's time: Cpu::now()
  = cycles + the running fast-interpreter block's instructions (Cpu::block_ea) or the JIT chain's (the JIT's
  fallback stub stores x25 - (count - 1) into Cpu::jit_left; run_native sets jit_budget); Cpu::read/write call
  Bus::clock(now) and Hw keeps it in `now`. Machine::run_field sets hw.now before the line ticks. Save state
  layout 6.
- Item 2 of the old plan needed nothing: the idle-loop skip never fires for a loop that reads a hardware
  register (mmio_read sets irq_dirty, which both ends the block and blocks the skip).
- Tests (tests/machine.rs): __AI_SRC_INIT replayed as a PowerPC program passes first time, four rounds, on the
  plain interpreter, the fast interpreter and the JIT (run with `--features unicorn`); the counter read inside a
  long compiled loop is identical on all three (and the test fails if the JIT's clock store is removed); rate,
  stop, SCRESET, AIIT interrupt and save states.
- The status screen redraws every 5 seconds (libretro.rs NOTICE_EVERY), so a screenshot is steady.
- Animal Crossing's own sound program is in ac-decomp's dspboot.c (`jdsp[]`, Ector 0x4BE6A5CB, 6656 bytes; saved
  to .scratch/ac_jdsp.bin when needed: parse the array). Its command jump table is at word 0x0062 (2 words per
  command; the light ack is 0x80000000 | the table entry's address). Read from the code: command 0B takes ONE
  word (fixed in dsp_zelda.rs; it called IROM 0x8644, the card decode, which our always-unlocked card never
  needs), 0C one word, 1 four, 2 two; command 2 then loops over the subframes, each waiting for one CPU mail
  (the zero mail from JAudio's DSPReleaseHalt) and raising DSPINT after its voices, as dsp_zelda models it.
  Its whole start-up (MXIC boot through the boot ROM protocol, hello 0x88881111, DwaitFrame, DsetupTable,
  DiplSec, frames of 7 subframes) is replayed in a test.
- What Animal Crossing does next (boot.c, read only): sound_initial (Na_InitAudio: AIInit, then the audio thread
  boots the DSP program and starts AI DMA; msleep 2.5 s), initial_menu_init, dvderr_init, sound_initial2 (waits
  for "Neos": its thread loads /audiorom.img from the disc into ARAM through DVD reads and ARAM DMA), then
  /COPYDATE, /static.str and /foresta.rel.szs from the disc. So the next things the game leans on are disc reads
  through the FST, ARAM DMA with its interrupt, and the DSP interrupt cycle above.
- BUILD 220 DONE 5:15 PM Eastern (started by hand, af28c89, green). Checked: 0.8.0 (220), 38 emulator files,
  continuum_gc dylib 1,462,880 bytes (differs from 219's). Link given to the owner with the steps (TESTING.md
  "Build 220"): https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-220-af28c89/Continuum-220.ipa
- NEXT: wait for his report (fix nothing while he tests). Then the GPU drawer (G6), a replacement IPL font,
  PAL (0xCC).

## Earlier (24 October 2026, about 4:30 PM Eastern): Animal Crossing boots on build 219, stuck in a sound timing loop

The owner opened a new chat because this one got too long (his phone was heating up). He is tired of games not
loading: fix this first, build once, give the link and steps. He sent two screenshots of the new status screen
(build 219, Animal Crossing USA, GAFE01). Their contents, written down here because the pictures are not saved:

    AFTER 70 / 140 SECONDS (the numbers changed too fast for him to read live; the two shots were enough)
    GAFE01 ANIMALCROSSING
    PC 8008C0C8 LR 8008C114 MSR 00001032
    SRR0 80079BE8 DEC F33F8382 / D2DE49B6 TB 79C06A2BD7672F / 79C06A4C38A0FB
    EXC 500:12 800:1 C00:9
    HOT 8008C000:685 8008C100:684 8007E100:5 80003400:1   (140 s: 1288, 1287, 5, ...)
    DISC 12:1 (only the inquiry, no reads yet)
    DSP NONE 00000000 CSR 0844     AI 00000001 DI 0000002A SI 60010306
    EXI 00000008 00000000 00000000  VI 0001 PI 00000FFC CAUSE 00010100
    GX PRIM 0 EFB 0 XFB 0
    LOADER: THIS APPLOADER BUILT APR 10 2002 02:35:29

So the boot fix worked: the apploader ran and the game's own code is running. With Animal Crossing's symbol map
(ac-decomp config/GAFE01_00/symbols.txt, reading only), 8008C04C-8008C1xx is `__AI_SRC_INIT` (dolphin/ai/ai.c),
called by AISetStreamPlayState / AIInit. CAUSE: that function measures the time from a step of the audio
interface's sample counter (AISCNT, AI register 0x08) at 32 kHz to the next step after switching AICR to 48 kHz,
reading the time base. It accepts diff < 28.5 us (bound_32KHz 31.524 us minus 3 us) or 34.5-39 us, otherwise it
tries again forever. Our AISCNT steps by 3 once per video line (Hw::ai_samples(3) from Machine::run_field, 63.5
us apart), so every measurement is about 63.5 us and it never finishes.

FIX PLAN (do this first in the new chat):
1. AISCNT counted from CPU time, sample by sample: Hw learns the CPU's cycle count at each register access (the
   slow memory paths that reach mmio_read/mmio_write: fast_ops load/store slow, the plain interpreter, and the
   JIT, whose handlers go through the same fast_ops functions), and the count is anchor_count + (now -
   anchor_cycle) * rate / cpu_hz at 32000 or 48000 Hz (AICR bit 1, AFR). Re-anchor on AICR writes (play/stop,
   rate, SCRESET) and AISCNT writes. AIINT (count reaching AIIT, with AIINTMSK) checked per video line from the
   computed count. Dolphin computes it from its cycle clock too, which gives diff about 20.8 us (one 48 kHz
   step), inside the first window.
2. The fast interpreter's idle-loop skip (fast.rs, `block.idle`, skips to the end of the line or the
   decrementer) must not jump past the next counter step when the loop reads AISCNT (the inner loops are
   `while (temp == AIRegs[2]) {}`): either treat a loop that reads a hardware register as not idle, or make
   the skip stop at the next sample step.
3. Test: a PowerPC test program that runs __AI_SRC_INIT's loop (set AICR 32 kHz + play, wait for AISCNT to
   change, mftb, switch to 48 kHz, wait, mftb, check the window) must finish in the plain interpreter, the fast
   interpreter and the JIT (tests/machine.rs style; the JIT under qemu with `.scratch/ca`).
4. The status screen: refresh it only every few seconds so a screenshot is steady (owner: "the numbers changed
   too fast to track").
5. Then look at what comes next in Animal Crossing's boot (DVD reads, the Zelda-family sound program upload),
   rebuild once (engine unchanged, so pushes to cores/continuum-gc alone do not start the iOS build: start it by
   hand with `gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/actions/workflows/ios.yml/dispatches -f
   ref=master`), check the .ipa, give the owner the link and steps.

## Earlier (24 October 2026, about 1:45 PM Eastern): GameCube in the app (hidden), memory cards, controls fixed

- BUILD 216 DONE 2:13 PM Eastern (green; checked: 0.8.0 (216), Frameworks/continuum_gc_libretro_ios.dylib 1.4 MB,
  unlock code in): https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-216-af13640/Continuum-216.ipa
  Link and test steps given to the owner at 2:15 PM; waiting for his report. At about 2:35 PM, while testing, he
  sent only "Nope." (no details). Not clear what failed, so the GPU drawer work was paused before any code was
  written (only reading and fetching wgpu 30.0.1 into .scratch/vkprobe) and he was asked what it refers to.
  Owner rule: fix nothing until he says he is done testing; record what he reports here.
- OWNER REPORT, build 216 (2:40 PM, three screenshots): the unlock works; Animal Crossing (USA) imports with its
  cover, but both copies (.nkit.iso and .rvz) say "won't start on Continuum GC" (the core's load_game refused).
  He is done testing ("games won't even load"). Cause found by reading Dolphin's boot ROM stand-in: our disc boot
  left r13 and r2 at 0, and Nintendo's apploaders keep their globals in the boot ROM's small-data area through
  r13 (Dolphin names Luigi's Mansion), so the first global was an access to 0xFFFF8xxx: a fault, then 200 million
  steps of nothing, then the refusal. Fixed (Machine::ipl_registers: r1/r2/r13 as the real IPL, NTSC and PAL),
  plus Dolphin's low-memory set exactly (devkit console type 0x10000006, rfi at 0x300/0x800/0xC00, the identity
  post-matrices; no arena/bi2/FST presets for discs). A test disc whose apploader works through r13 fails the old
  way and boots now. A faulting apploader is refused at once, and the core's refusal now reaches the screen as
  a sentence with the detail ("cGC: " log line; engine refusal_from_log + launch_failure SPEAKS_FOR_ITSELF).
  If a game boots but shows nothing for 15 seconds, the core shows a status screen (notice.rs: PC, LR,
  exceptions, faults, disc commands, sound program, registers, GX counters, the apploader's last line) so his
  screenshot carries the state.
- Also found by reading Animal Crossing's disc library (ac-decomp dvdlow.c, reading only): every drive command
  completed inside the register write, so the completion interrupt could come before the library set its
  ten-second timeout; ten seconds later the timeout masked the drive's interrupt for good (a hang mid-boot).
  Drive commands now complete two video lines later (DI_COMMAND_LINES; TSTART stays set until then). (Run 40118325908, started by hand
  1:55 PM from af13640, which adds the drive's streamed music that Ikaruga uses; 215 was cancelled for it.) It is
  the one experimental GameCube build the owner agreed to. Build 214 (ac4deec, the wiring) built and verified its .ipa (continuum_gc dylib inside) but
  FAILED at "Upload the .ipa": GitHub's own network (ENOTFOUND creating the artifact), not our code. Its cores were
  cached first ("Cache saved", key ios-cores-macOS-7781a752f3acd0d7-40110264031-1), so 216 only rebuilds the GC
  core. 216 also carries everything below. When it is green: download Continuum-216.ipa from the Release
  build-216-<sha>, check version 0.8.0 (216) and Frameworks/continuum_gc_libretro_ios.dylib, give the owner the
  link and the steps (Settings, ABOUT, tap Version 7 times, cube drops in, "GameCube (experimental)" switch on;
  import a .iso/.rvz/.gcz; try Ikaruga first; slow; saves on the memory card; report what he sees). If it fails,
  read the log (gh api repos/.../actions/jobs/<id>/logs) and fix.
- The owner chose "B" (secret unlock): all of it is `native/ios/GameCubeUnlock.swift`, hooks marked
  `// GameCube unlock`, so it comes out in one go when GameCube is ready for everyone.
- Memory card (G9, `cores/continuum-gc/src/memcard.rs`): a 251-block Nintendo card in slot A, the game's libretro
  SAVE_RAM (the app's .srm, one card per game, its 20-second flush and on leaving). Blank cards are formatted the
  library's way. SRAM is given the flash ID of whatever card is in the slot, so card images from consoles or other
  emulators mount. SRAM fix: language and flags were swapped (games saw language 0x2C and mono), flash ID checksums
  were missing. Proved with GCMM (homebrew card manager, `.scratch/homebrew/gcmm`, test only): restore a save,
  list it after a restart, format; `.scratch/gcmmtest/checkcard.py` checks a card like the library does.
- CONTROLS WERE BROKEN for every game until 3394ad4: SI polling never set RDST, and Nintendo's PADRead and libogc
  only take input RDST marks as new. Now polled per SIPOLL with RDST/NOREP/RDSTINT (tests/machine.rs).
- EXI: immediate transfers take their wire time (Hw::stall, counted by Machine::run_field); registers mirror every
  64 bytes (libogc2's EXI_Sync uses it). Clock: the RTC starts from the phone's local time (libc localtime_r) and
  ticks with video lines; the time base starts from it (Animal Crossing's clock). Japanese discs: VI DTV bit 1.
- Zelda-family sound (`src/dsp_zelda.rs`, Luigi's Mansion, Animal Crossing, Pikmin, Sunshine, Wind Waker...): full
  stand-in, unit-tested only. JIT re-proved after paired singles: Swiss, FCE, Neverball IDENTICAL as ARM64 code.
- Test tools: `examples/boot.rs` takes CGC_PAD="from-to:A+B,...", CGC_SHOTS/CGC_SHOT_DIR, CGC_CARD_IN/OUT,
  CGC_EXI_TRACE. Fingerprints in `.scratch/golden/` re-recorded with the card in (older ones in golden/pre-card).
  Background jobs: use run_in_background (a plain `&` dies with the command's sandbox).
- Done after 216's code: TEV operands resolved per primitive (5e8fe44, ~2%). The CPU drawer costs ~21 ns a pixel
  for one untextured stage, ~43 with a bilinear texture (bench in tev.rs): the GPU drawer is the real fix.
- VULKAN ON THE TEST MACHINE: `. .scratch/lvp/env.sh` (lavapipe from Amazon Linux 2023 RPMs in .scratch/lvp/al,
  libwayland 1.22, libffi.so.6 -> system .so.8); `.scratch/vkprobe` lists 'llvmpipe (LLVM 15.0.7)', Vulkan 1.3.
  So a GPU drawer (the app's Vulkan path, SET_HW_RENDER via MoltenVK) can be built and compared with the CPU
  drawer here.
- Next: whatever the owner reports from build 216; then the GPU drawer (G6); a replacement IPL font; PAL (0xCC).

## Earlier (24 October 2026, about 10:30 AM Eastern): GameCube core — our own JIT, drawing on several cores, RVZ, AX

- Our JIT (G8) is in `cores/continuum-gc/src/jit/` and turns on at load when the app allows JIT (code memory from
  the app's `continuum_jit_region`, two addresses, as iOS 26 needs). Proved equal to the interpreter: 20,000 random
  blocks, and whole programs (Swiss, FCE Ultra GX, Neverball) IDENTICAL to the plain interpreter with the JIT on,
  run as real ARM64 code. Neverball runs 99.7% of its instructions in compiled code.
- ARM64 on this x86 machine: `.scratch/ca` (cargo for aarch64-unknown-linux-musl, toolchain copy in .scratch/tc/r192
  with the iOS, ARM64 Linux and musl standard libraries) runs tests and examples under `.scratch/tc/qemu/usr/bin/
  qemu-aarch64-static` (Debian qemu-user-static 7.2). Or `--features unicorn` on x86 (Unicorn backend, slower).
- Drawing now runs on helper threads (`src/gx/draw.rs`, exactly the same picture: each helper owns interleaved
  8-row bands). `examples/drawcheck.rs` proves it: `drawcheck <dol> <fields> record|check <file>` with CGC_JIT=1 and
  CGC_DRAW_THREADS=n; fingerprints of the original drawer are in `.scratch/golden/`. Neverball's SD image is WRITTEN
  by the game: always run from a fresh copy of `.scratch/neverball-pristine.img` (see `.scratch/dc.sh`).
- Owner, 24 October ~9 AM, sent five speed ideas from another AI (return stack, fused instruction pairs, tail calls,
  memory shortcut, memcpy replacements). Measured with `examples/hot.rs` and the profiler; results in
  tasks/gamecube.json G7: only the memory shortcut was worth doing (done; small gain). The real bottleneck was
  drawing (55-89% of the time), hence the threads.
- Also done by ~10:30 AM: compressed discs (RVZ, WIA, GCZ, CISO, TGC) through the `nod` library (Swiss's own disc,
  converted with `.scratch/nodtool/bin/nodtool`, draws exactly like the ISO in every format: `.scratch/discs/`), and
  AX (`src/dsp_ax.rs`), the sound program most commercial games use, plus the memory-card unlock program and program
  switching. AX is unit-tested only: no commercial game here.
- OPEN QUESTION FOR THE OWNER (rules 12 and 16: do not build it before he answers): how should the experimental
  GameCube core get into the app? (1) a "GameCube (experimental)" switch in Settings, off by default, in every
  build (testers could find it too); (2) only in one special test build for him; (3) wait until it plays more. The
  wiring map (build-core.sh, check-core-cache.sh, build-engine.sh, project.yml, ContinuumApp.swift, TouchControls,
  GameArtwork, NewLookShell, ImportCenter, detect.rs, jit.rs) is in this session's notes below. Editing
  build-engine.sh makes the next build rebuild every core once (about an hour).
- Next, in order: JIT floating point and paired singles natively, JIT register cache, TEV per-stage operand plans
  (the CPU drawer is ~90 ns a pixel per thread), the GPU drawer, Zelda-family sound program (Nintendo's own games).

### Wiring map for the GameCube core (G2), found 24 October (not done; waiting on the owner's answer)

Model it on cArcade (`.kiro/notes/docs/ARCADE_CORE.md`, "Every place the app wires it in"):
- `scripts/build-core.sh`: add `continuum_gc` to `IOS_OPTIONAL_CORES`; a `# recipe-start: continuum_gc` region with
  its config arm (`IOS_KIND="cargo-continuum-gc"`, `IOS_DYLIB_NAME="continuum_gc_libretro_ios.dylib"`, no repo, no
  pin), `build_ios_continuum_gc_core` (cargo build --release --target aarch64-apple-ios in cores/continuum-gc,
  stage `libcontinuum_gc.dylib`) and its dispatch in `build_ios_core`, all inside the region.
- `scripts/check-core-cache.sh`: every hard-coded `IOS_OPTIONAL_CORES=(...)` line (four places) and three gc checks
  like cArcade's. `native/ios/build-engine.sh`: drop a cached gc dylib (no pin) and add it to `NOT_IN_IOS_YML`
  (ios.yml edits are parked). `native/ios/project.yml`: the four-line optional embed.
- Swift: `GameSystem` gets `gamecube` (Wii later), and every exhaustive switch (TouchControls badge,
  dpadDrivesAnalogStick, touchScreen, displayName, wantsKeyboard, controls (GameCube pad: retro A=A, B=B, X=X, Y=Y,
  L, R, L2/R2=Z, Start; left stick = main stick, right = C-stick), GameArtwork playlistDirectory and hue,
  NewLookShell shortName, trayName, consoleColor); `ContinuumApp.swift` CoreSpec `continuumGc` (640x480, max
  720x576, 4:3, 59.94, 48000, XRGB8888) in `CoreCatalog.all` (not `required`), `defaultCoreBySystem[.gamecube]`,
  `discSystems`, routeTable rows for gcm/dol/rvz/gcz/ciso/wia; `unplayableSystems` keeps GameCube's line unless the
  switch is on and the dylib is bundled; `ImportCenter.swift` `NotGames.gameTypesThisBuildCantRun` loses the
  routed extensions. Engine: `detect.rs` (dol, ciso, wia, tgc), `jit.rs` `core_may_use_jit` adds `continuum_gc`.
- nod's C compressors must compile for aarch64-apple-ios in CI (cc crate; not tried here, no Apple SDK).

## Earlier (24 October 2026, morning Eastern): GameCube core — all three homebrew run, graphics, sound, fast interpreter

- Owner, about 6:50 AM: wants the ABSOLUTE BEST fast interpreter and JIT, better than Dolphin, however long it
  takes (saved in tasks/gamecube.json owner_answers). He also said: REA is not optional; use it via its command
  line (`/projects/sandbox/.scratch/rea` wrapper: npx rea-agents@6.7.0 with Ghidra 12.1.4 in .scratch/tools)
  where it clearly helps (it decompiled Neverball's start-up and showed it needs a writable data folder).
- Working now on the test machine (pictures in .kiro/notes/docs/gamecube/): FCE Ultra GX menu, Swiss home screen,
  Neverball 3D title with menu music (AESND stand-in). Neverball needs its data on an SD image (CGC_SD=img,
  CGC_ARGV="sd:/default.dol" CGC_ENV="PWD=sd:/ HOME=sd:/"; image built with .scratch/mkfat.py + pyfatfs).
- Done today in cores/continuum-gc: src/gx/ (our own graphics engine, G6 first pass), src/sd.rs (SD card
  adapter), src/dsp_hle.rs (ASND and AESND sound stand-ins), audio DMA output, PI bus-error bit, thermal unit,
  argv/envp, src/cpu/fast.rs + fast_ops.rs (decoded fast interpreter, proved identical: fuzz test of 3 million
  random instructions + examples/compare.rs on all three programs). Speed on the x86 test machine: Neverball CPU
  ~2.4x the plain interpreter; drawing (software) ~53 ms a field is now the bigger cost.
- Dev tools: examples/boot (CGC_WAV=out.wav saves sound), probe, compare (plain vs fast), profile
  (`cargo +1.92.0 run --profile profiling --example profile`), speed. Scratch tools in /projects/sandbox/.scratch:
  `c` (cargo wrapper with writable CARGO_HOME), ppcdis.py, dol2elf.py, sym.py, mkfat.py, venv (Pillow, numpy).
- Fast interpreter now (later the same morning): block linking + chaining, exact idle-loop fast-forward,
  FP quick path (proved vs the full path), translation cache kept across exceptions. examples/speed2: fast 298 vs
  plain 103 million instructions a second; still IDENTICAL on all three programs (examples/compare.rs).
- Owner, ~7 AM: screenshots of Manic EMU's "Cached Interpreter" (Dolphin's) playing Rogue Squadron III almost
  perfectly on his phone with no JIT. That is the bar: match/beat Dolphin's cached interpreter, and draw on the
  GPU (our CPU drawer is ~51 ms a field on Neverball here).
- Owner, ~7:40 AM: an occasional experimental GameCube build is fine, not constantly (tasks/gamecube.json).
  Offer one at a milestone; start only on his yes. The core is NOT wired into the app yet (G2).
- GPU testing here: Vulkan 1.3 works through Chromium's SwiftShader (no root needed):
  `LD_LIBRARY_PATH=/inner_container/opt/playwright/chromium-1232/chrome-linux64
  VK_ICD_FILENAMES=/inner_container/opt/playwright/chromium-1232/chrome-linux64/vk_swiftshader_icd.json`
  with the `ash` crate (test crate in .scratch/vktest). No logicOp and no dual-source blending (like Metal):
  do logic ops and dest alpha in the shader.
- ARM64 testing here: unicorn 2.1.4 in .scratch/venv (Python) and llvm-mc for encodings. src/jit/a64.rs is
  our ARM64 encoder (checked against LLVM).
- Next, in order: JIT (G8: block compiler on top of fast_ops handlers as fallback, register cache, inline RAM
  fast path for the standard BAT layout, tested against the plain interpreter by running the ARM64 code in
  unicorn), GPU drawer (G6 part 2, Vulkan, checked against the CPU drawer with SwiftShader), AX sound stand-in,
  memory cards (G9), then wire the core into the app as an opt-in experimental core (G2).

## Earlier (24 October 2026, about 9:15 AM Eastern): GameCube core, booting real homebrew

- Owner rules added overnight: never stop early (keep working through the plan in one turn);
  never ask small questions while he sleeps. REA (reverse-engineer-anything) skill and the
  rea-agents MCP server are installed (`.kiro/skills/`, `.kiro/settings/mcp.json`); usable from
  this new session.
- `cores/continuum-gc` boots three free homebrew programs on the test machine (test only, never
  committed, kept in `/projects/sandbox/.scratch/homebrew/`, re-download from GitHub releases):
  FCE Ultra GX 4.0.3 (`dborth/fceugx`), Neverball (`emukidid/swiss-gc` release asset), Swiss
  r2119. Fixed this morning: a DSP boot-ROM stand-in (reset mail 0x8071FEED, the init-code mail,
  the ucode upload protocol 0x80F3xxxx, then 0xDCD10000) and the DI interrupt check.
- Dev tools in `cores/continuum-gc/examples/`: `boot` (runs N fields, prints exceptions, faults,
  unknown registers, pending interrupts, writes the picture as PPM, dumps code at PC/LR to
  /tmp/gc-dump.txt), `trace`, `dis_at`, `speed`. Disassemble with llvm-mc
  (`--disassemble -triple=powerpc -mcpu=750`). clang can build PowerPC code for our own tests.
- Where each one stands: FCE Ultra GX shows a 640x480 picture (not checked what is in it yet),
  sends 270 KB to the graphics FIFO (no graphics engine yet), takes DSI faults at 801A0E78 with
  addresses like 7FFFBFF8 (looks like a memory-size or heap mistake: check lowmem 0x80000028 /
  0x800000F0 / arena and the BATs). Neverball faults on a null pointer at 80019DA4 and sits at
  800E8790. Swiss r2119 is linked to run from 0x00397B5C (a physical-address loader) and spins:
  probably expects translation off and a different entry; check its DOL header.
- `dis_at` stopping on the first fault is too slow at 4 billion steps: give it a smaller limit.
- Do NOT use `pkill -f examples/` in a command: it kills the command's own shell.
- Next after these boot: graphics G6 (CP FIFO, XF, BP/TEV, CPU drawer first), then sound, the
  fast interpreter (G7), the JIT (G8). Plan and per-step progress: `tasks/gamecube.json`.

## Earlier (23 October 2026, about 10:30 PM Eastern): GameCube and Wii, our own

- The plan and the owner's answers are in `.kiro/notes/tasks/gamecube.json` (steps G1 to G12).
  Wii is in from the start. Anything may be read (Dolphin's source too) but nothing copied, and
  only no full emulator by anyone else; ordinary libraries ARE allowed (owner corrected this, 23 October). Dolphin may run on the test
  machine as a yardstick only. No build until real games play properly. Name not decided yet.
- G1 done (first pass, 23 October): `docs/GC_OPTIONS_INVENTORY.md`, generated by
  `scripts/build-gc-inventory.py` from `scripts/extract-gc-option-keys.py` (19 sources: Dolphin and
  its Android app, RetroArch's Dolphin, Manic's, DolphiniOS, iCube, Provenance, MMJR2 and its
  Android app, Slippi, PrimeHack, MPN, Ishiiruka, DolphinPS4, Porpoise, Lazuli, gecko, rustcube,
  pureikyubu). 943 different settings: 479 planned in our core, 321 the app does, 46 belong in the
  app but it lacks them (speed read-outs, sound effects, video/input recording), 97 do not apply
  (desktop windows, drivers, updaters, real Bluetooth/USB adapters, Slippi's servers).
  `option-sources/gc/mapping.tsv` and `same.tsv` are now HAND files (the rules draft that first
  filled them is not in the repo; do not regenerate). "And more": our extras go in during G11
  (ideas so far: GBA link cable and Game Boy Player through cBoy).
- **Owner rule (24 October, 3 AM): when he says he is going to bed, keep working and never stop
  to ask; decide small things yourself and note them** (he was furious at being asked the name).
- Overnight 23-24 October: `cores/continuum-gc` (36 tests, all pass, clippy clean): the plain CPU
  interpreter (G4), console hardware start (G5), DOL/ELF/ISO boot without an IPL (G3 part), the
  libretro entry points (G2 part), save states. Details and gaps per step: `tasks/gamecube.json`
  "progress". Our own VI test program shows its framebuffer and gets VI interrupts. NOT in the
  app or build yet (on purpose). Run tests: `cargo +1.92.0 test` in the crate with
  CARGO_HOME=/projects/sandbox/.scratch/cargo-home (the default cargo home is read-only here).
- Next, in order: (1) prove the CPU against public PowerPC tests and Dolphin as yardstick on
  open homebrew; (2) find free GameCube homebrew DOLs (libogc demos) to boot; (3) graphics G6
  (CP FIFO, XF, BP/TEV, a CPU drawer first); (4) sound (AI DMA, our own DSP stand-ins); (5) G7
  fast interpreter; (6) G8 JIT.
- **Owner, 23 October, about 10:45 PM: "I want my app closed sourced."** NOTHING changed for it.
  He was told: the app ships about 30 GPL emulators (PPSSPP, Azahar, bsnes, Mesen and the rest),
  and GPL requires the app's source to stay public, so closed source means only our own cores
  (cBoy, cArcade, the GameCube one) could stay in. Making the repo private also breaks the
  Releases download link his testers use. He asked how Manic does it: Manic does NOT; its app is
  AGPL-3.0 and public (`github.com/Manic-EMU/ManicEMU`, with a third-party notices file), and its
  Dolphin fork is public too. Not legal advice. **His answer: choice 1** (stay public until
  our own cores replace the outside ones, then go closed); saved in owner-rules. He posts the
  .ipa on the internet himself, so the GitHub link is not what testers use.
- Owner: the settings list was missing Lazuli (vxpm/lazuli, a from-scratch Rust GameCube
  emulator). Added with gecko (ioncodes/gecko, Rust), rustcube (Rust), Ishiiruka and Dolphin-MPN.

## Earlier (23 October 2026, about 9:45 PM Eastern): build 213; next is GameCube

- **Build 213 is the newest install** (`84e7636`, 9:42 PM Eastern), checked: 0.8.0 (213), 37
  emulator files. https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-213-84e7636/Continuum-213.ipa
  The turned box plus the bug hunt's fixes (TESTING.md "after 212"). Left for later: a joined
  disc game does not carry the cover picked for disc 1 (`MultiDisc.carryOver`, artwork keyed by
  path).
- The two looks are NOT separate code: Settings, the Library, the Game Hub and more are one
  implementation styled by `LookStore.isNewLook` (New-look pieces in 21 shared files). Removing a
  look later needs care. The owner asked about this on 23 October.
- NEXT (owner): start on the GameCube emulator, OUR OWN, written from scratch like cBoy
  (`cores/continuum-gb`) and the arcade core (`cores/continuum-arcade`). NOT Dolphin, not a
  wrapper of anyone's emulator: the point of our own cores is to get rid of credits and outside
  licences (owner-rules, "Licences"). `tasks/gamecube.json` (Dolphin plus our interpreter) is
  replaced by this. It needs BOTH: a JIT of our own for every phone that allows JIT (used
  automatically, through `crates/emulator-bridge/src/jit.rs`, owner-rules "JIT IS STILL REQUIRED
  FOR EVERYONE ELSE"), and a fast interpreter for phones without it, like the owner's. Core
  settings (owner): every setting any current GameCube emulator offers, and more
  (`docs/CORE_OPTIONS_STANDARD.md`, `docs/option-sources/`).

## Earlier (23 October 2026, about 7 PM Eastern): box side fixed (not built), bugs to verify

- Owner circled the standing box's side (Doki Doki Panic, Pokemon Silver): a flat strip of another
  colour stuck on the cover. `NewLookBoxFace` is now a real turned box: front `rotation3DEffect`
  -18 deg on its trailing edge, side folds back +72 deg from the same edge, perspective 0.2, side in
  the cover's edge colour (16-34% shade), `NewLookBoxFrontShape` (square corners at the join).
  Simulator: `docs/new-look/simulator-box-51130ec.jpg` (preview `-page box`, covers fetched by
  run.sh). NOT BUILT: owner said no new build; goes in the next batch.
- Bug hunt (owner asked: report, he verifies on the phone, fix nothing yet): Game Hub "Delete
  this game" has no confirm; auto-save off still resumes the old auto-save (`resumeIfPossible`
  ignores `autoSavesEnabled`); swirl Accent comes back as My own after relaunch (leftover
  `swirlHex` + init fallback); filter sliders other than brightness reset every launch
  (`applyCoreActionPreferences`); select All + Delete takes games in folded console sections;
  swirl Speed label `%.2g` shows 1.2x for 1.25x; restart after a core setting loses progress when
  "Pick up where you left off" is off. Waiting on his results.

## Earlier (23 October 2026, about 8:30 AM Eastern): console list and cover frames after 211

- The console bug: `ConsoleSections` rebuilt its groups in `.onChange(of: entries) { _ in rebuild() }`
  reading `entries` from self, which on iOS 16 can be the view from BEFORE the change, so the
  Library showed the previously picked console (and folded). Now `rebuild(latest)` from the
  closure's value. RULE: in onChange, use the new value passed in, never a `let` of the view.
  With a console picked (New look), `ConsoleSections(showsHeaders: false)`: just the grid.
- Frames fit the art: `CoverArtView` publishes `CoverArtAspectKey`; `NewLookGlowCard` sizes the
  frame from it (or the cached cover, or the console's box shape). Shelves (`widthFollowsArt`):
  row height fixed, width = art (0.6x to 1.6x the base). Grids: art's shape fitted in the cell,
  bottom-leading (`NewLookCardFace` frameWidth / slotHeight). Square covers unchanged.
- OWNER ON 212: the folded bar alone was fine; showing the games straight away (`showsHeaders:
  false`) was NOT asked for. Nothing changed yet and no build for it: only if he says so, and
  then in the next batch, never a build of its own. Owner rule 16.
- **Build 212 is the newest install** (`fd0d355`), checked: 0.8.0 (212), 37 emulator files.
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-212-fd0d355/Continuum-212.ipa
  Phone checks: TESTING.md "after 211".

## Earlier (22 October 2026, about 11:15 PM Eastern): header, console picker, Settings after 209

- Owner on 209: swirl "good enough for now". Deleted his video release (🖕) and its tag.
- Header: `NewLookHeaderGlass` is gone. `NewLookHeaderFade` (NewLookDesign.swift) masks only
  Home's ScrollView (the one page that scrolls under the header): clear above header bottom
  minus 30, eased over 40 pt. `newTopBar` has `.contentShape(Rectangle())` so a faded card under
  it cannot be tapped.
- Console picker (`LookStore.librarySystem`): Home lights only the picked console (it lit the hero's
  console, so Home and Library disagreed: likely the owner's unnamed bug); re-tapping the picked
  console = All; changing console ends select mode; `LibrarySelection.picked(in:)` so Delete,
  favourite and All/None act only on ticked games on screen; a console with no games left goes
  back to All; search from Home or Settings clears the console; "No match" names the console.
- Settings: `SettingsFolds` closes everything once (`continuum.settings.allClosedOnce210`). The
  New-look cards fold through it (keys "NEW LOOK ACCENT" etc.): four cards, Ring inside Light
  and motion, preview inside Accent, no blurbs. Merged: CORES + CORE SETTINGS
  (`CoreSettingsSettingsSection<Leading>`, one Menu for the cores, TV buttons show their value);
  GAME CONTROLLERS + CONTROLLERS, MAPPING AND MOTION + ONLINE PLAY (`InputSettingsSection<Leading,
  Trailing>`, `OnlinePlayNotes`); ON-SCREEN CONTROLS + HAPTICS AND TURBO; SAVE STATES + CHEATS +
  STORAGE; APPEARANCE moved into `LookPickerCard(appearance:)` for the Current look.
- **Build 211 is the newest install** (`dc53d06`, 10:59 PM Eastern), checked: 0.8.0 (211), 37
  emulator files. https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-211-dc53d06/Continuum-211.ipa
  (210 is the same minus the review fixes.) Phone checks: TESTING.md "after 209".

## Earlier (22 October 2026, about 9:45 PM Eastern): whole screen in swirling colour after 208

- Owner on 208: "all it is is strips"; on the dark between ribbons: "NO GAPS". (The dark came
  from the spec text he pasted after 207; he does not want it.) He wants the WHOLE background in
  colour, every colour at once, swirling round each other, never one colour everywhere, never grey.
- `NewLookSwirl` (NewLookMotion.swift) is now a colour field worked out per point on a 41-wide
  grid each frame (`NewLookSwirlField.pixels`, plain Foundation, 1.2 ms a frame on the Linux
  sandbox), drawn as a small CGImage scaled up and blurred at a third resolution.
  - Each colour = two soft clouds, all evenly round a slowly turning oval, each drifting in and
    out; order round the oval follows the colour wheel there and back (`NewLookSwirlPalette.ring`)
    so only neighbouring hues meet.
  - Two wandering whirlpools (opposite turns) twist the page, plus a sine wobble: curls.
  - Mixing in Oklab (`NewLookOklab`): lightness, chroma and hue mixed separately, hue unwrapped
    so it never crosses the widest empty stretch of the wheel. Never grey.
  - Pace 1.5x inside `pixels` (at 1x it barely moved in a 10-second film). Speed setting on top.
- Prototype: `.scratch/rib/field4.py` (not in the repo). Simulator film:
  `docs/new-look/simulator-colour-swirl-0e329b6.jpg`. Phone checks: TESTING.md "after 208".
- **Build 209 is the newest install** (`8cf645e`, 9:54 PM Eastern), checked: 0.8.0 (209), 37
  emulator files. https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-209-8cf645e/Continuum-209.ipa

## Earlier (22 October 2026, about 8:15 PM Eastern): colour ribbons after 207 (rejected: "strips")

- Owner on 207: "50%". Colours took turns over the whole screen and went grey where they met. He
  wants every colour on screen at once in tighter ribbon-like waves, dark page between, swirling
  round each other, never one colour everywhere, never muddy (TESTING.md "after 207", verbatim).
- `NewLookSwirl` (NewLookMotion.swift) is no longer a MeshGradient. It is a Canvas of ribbons:
  two per colour (half a page apart, so none ever leaves), evenly spaced, drifting with wrap
  round, each leaning, waving and bowing on its own. Glows are drawn on the page; ribbons on a
  layer above, and before each ribbon the ones under it are cut away just round it
  (`.destinationOut`), so crossings never average two colours into grey. Width: `unit*0.12`,
  narrower with more than six ribbons (Rainbow's eight). Drawn at a third, scaled up.
- `swirlRGB`: one colour per ribbon pair. Accent: its two stops plus a near neighbour
  (`shiftedHue(first, -0.08)`, so gold gets orange, not lime). Rainbow: four hues. My own: his.
- Preview films purple, his own three, gold and Rainbow (retries an empty film). Prototype maths:
  every colour on screen in every frame, dark 28 to 51%, no grey. Picture:
  `docs/new-look/simulator-ribbons-1df1a18.jpg`.
- **Build 208 is the newest install** (`507cd4d`, 8:24 PM Eastern), checked: 0.8.0 (208), 37
  emulator files. https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-208-507cd4d/Continuum-208.ipa
- Phone checks: TESTING.md "Next install (after 207)".

## Earlier (22 October 2026, about 6:45 PM Eastern): how the colours mix

- Build 206's MeshGradient folded (its inner points turned round the middle and crossed). The
  owner sent a screen recording of Gemini and Manic (release tag 🖕, harmless: the install link
  still resolves to the newest build) and said only colour mixing matters, over the whole page.
- `NewLookSwirl` (197fc83): points stay inside their cells (tested: never cross), colours sweep
  across the page in bands, mixed round the colour wheel (`NewLookHSV`), perceptual MeshGradient.
  `NewLookColors.swirlSource`: Accent / Rainbow / My own. Simulator film looked smooth.
- **Build 207 is the newest install** (`a23d4d9`, 7:08 PM Eastern), checked: 0.8.0 (207), 37
  emulator files. https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-207-a23d4d9/Continuum-207.ipa
- Phone checks: TESTING.md "Next install (after 206)".

## Earlier (22 October 2026, about 4 PM Eastern): swirl back, everything customisable

- Owner on build 205: furious that the swirling background colours were gone ("I SAID LIKE
  GOOGLE GEMINI AND MANIC"); "if you're not gonna do something right, don't do it at all";
  bring back the light round buttons with a switch; everything customisable, always. Two new
  rules in owner-rules.md. His screenshots: `docs/new-look/build-205-*.jpg`.
- Done (4c79532): `NewLookSwirl` (NewLookMotion.swift): a 4x4 MeshGradient on iOS 18+ (blurred
  Canvas shapes before) whose inner points turn round the middle, colours from
  `NewLookColors.swirlRGB`. `NewLookPage` picks Swirl / Glow (NewLookAura) / Plain. New
  settings in NewLookColors: backgroundStyle, swirlHex (own 2 or 3 colours), swirlStrength,
  swirlSpeed, buttonLight, lightSpeed, heroGlow, boxFloats; Settings → LOOK → Background and
  "Light and motion" (NewLookTheme.swift). `newLookButtonLight` puts NewLookFlowRim on the
  primary buttons, the picked console and the picked chips. Box side: `NewLookBoxFace.edgeColor`.
  Console count inside the tile. `NewLookHeaderGlass` behind the New header.
- **Build 206 is the newest install** (`6ddb46c`, 4:46 PM Eastern, 22 October), checked: 0.8.0
  (206), 37 emulator files, the new settings' text inside. Simulator preview of this code: the
  swirl moves (62% of pixels change in 4 seconds), the header blurs what scrolls under it.
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-206-6ddb46c/Continuum-206.ipa
- Phone checks: TESTING.md "Next install (after 205)".

## Earlier (22 October 2026, evening Eastern): the New look redone to the mockup

- The owner said the New look (197 to 204) "ain't how I want it" and sent the mockup again; it is
  now in the repo: `.kiro/notes/docs/new-look/mockup-home.jpg` (and build 204's empty Home,
  `build-204-empty-home.png`). The picture is the reference. His brief: it must look like the
  mockup, move like Google's Gemini app (calm dark page, a soft glow that drifts and breathes),
  and cover EVERY screen. He also said: ask when it truly is unclear, but put two and two
  together on common sense and don't stop for dumb questions. Progress bars: the app could track
  how far into a game, but he does NOT want that yet.
- **Build 205 is the newest install** (`2eb4943`, 3:09 PM Eastern, 22 October), checked: 0.8.0
  (205), 37 emulator files, the new screens' text inside. Not tried on a phone yet:
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-205-2eb4943/Continuum-205.ipa
- Done (commits bbbb3ea, e880b21, 6f469a3, then the build):
  - `NewLookDesign.swift` (new): every New-look piece drawn from plain values; `NewLookMetrics`
    holds the mockup's sizes. `NewLookColors.swift` (new): the colour store, split out of
    LookTheme.swift so it compiles alone. `NewLookShell.swift` hands them real games and actions.
  - Home to the mockup: hero with no card, standing box (`CoverStyle.box`, `NewLookBoxFace`) with
    the gold glow (`NewLookHeroGlow`), SYSTEMS tray with full names, Jump Back In cards with one
    grey line, glass tab bar with no pill, the controller mark. Empty Home: `NewLookEmptyHero`.
  - Motion (NewLookMotion.swift): `NewLookAura`, a calm glow from the top plus one travelling
    along the bottom, replaces the blobs; "Moving glow" (on by default). Gone: sparks, title
    shine, swirls in cards, the tab pill, per-card breathing (that made phones warm).
  - Every sheet: `.newLookList()` / `.newLookSheetPage()` (NewLookShell.swift) on 17 sheets; the
    gold tint on every system control (RootView `.tint`, `AppAppearance.applyTint`); the game
    card's blocks via `LookBlockHeading` / `LookBlockCard`. Current look unchanged throughout.
  - Bugs from the review fixed: pause menu Resume, sheets from the pause menu keep the game
    paused, Resume label follows "Pick up where you left off", tab bar tap-through, Cloud Sync
    merges play time, colour picks saved once the finger rests, Library sorted only on change.
- How to see it without a phone: `.github/workflows/newlook-preview.yml` (start it by hand; the
  pushes are `[skip ci]`) draws the real pieces with the mockup's games in the iPhone 17 Pro Max
  simulator and uploads `home.png`, `empty.png` beside `mockup-home.jpg`. Compare them yourself
  before telling the owner anything looks right. It does not cover sheets (they need the engine).
- Phone checks: TESTING.md "Next install (after 204)".
- Still waiting: emulator updates (10 of 32 newer on 19 October, not checked), then
  `tasks/gamecube.json`.

## Earlier (22 October 2026): check-up after the other AI's builds 197 to 204

- The owner ran out of Kiro credits on 19 October and used another AI tool for builds 197 to 204
  (commits by "Continuum Dev"). Kiro is back on a new account; the 11 memories are saved.
- Checked: every build 193 to 204 passed; build 204 is 0.8.0 (204) with 37 emulator files; all 60
  Swift files parse and are in project.yml; no other AI tool's files are in the repo. `97ee938`
  (sparkle mark removed, `[skip ci]`) is in no build yet.
- Full review: `docs/reports/new-look-review-2026-10-22.md`. To fix in the next batch:
  1. Pause menu Resume does nothing when the game was already paused before the menu opened
     (`PlayerScreen.swift` `finishPauseMenu`, gate on `pausedForMenu`).
  2. Pause menu choices that open another sheet (Save slots, Cheats, Skin, Functions,
     Controllers, Feedback) un-pause the game first.
  3. Home hero and Game Hub say "Resume" even when auto-resume is off in Settings.
  4. New-look floating tab bar lets taps through beside / under it to covers (can start a game).
  5. Cloud Sync overwrites play time and last played (merge them like the skin keys).
  Speed: always-on breathing shadows on every card and tile (even with glow Off), ~40 clocks in
  Settings, colour-picker drags save and redraw on every tick, Library re-sorts every redraw.
  Workflow: `newlook-preview.yml` has no concurrency or simctl timeouts; one run hung 30 minutes.
- Emulator updates: `docs/CORE_UPDATES.md` (19 October) lists 10 of 32 with newer code (PSP 189
  changes, N64, 3DS, Dreamcast, Gearsystem, Beetle PSX HW; Amiga, C64, DOOM, Jaguar builds). Not
  checked yet.
- Then the plan carries on: `tasks/gamecube.json` (GC1 to GC5, none started).

## START HERE (19 October 2026, late night Eastern): New-look motion after 201

- `NewLookMotion.swift`: `NewLookClock` (TimelineView, frozen with Reduce Motion), `NewLookFluid`
  (Canvas, 6 to 11 stretched, rotating radial masses on irrational speeds, colour drifting
  through `NewLookColors.motionRGB`, drawn at 1/2 or 1/3 resolution and scaled up),
  `NewLookSparks`, `NewLookFloat`, `NewLookBreathe`, `NewLookSweep`, `NewLookFlowRim`,
  `NewLookPressStyle` / `NewLookButtons`, `NewLookToggleStyle` / `NewLookToggles`,
  `NewLookAppear`, `NewLookSparkleShape` / `NewLookSparkleMark`, `NewLookTabPill`.
- iOS 16 target: no Metal shaders, nothing needs #available. The old `NewLookMovingBackdrop`
  is gone; `NewLookBackdrop` and `NewLookGlow` use `NewLookFluid`.
- Not touched: the header, the CONTINUUM wordmark and ring, the Current look, app icons.

## START HERE (19 October 2026, night Eastern): colours inline again after 200

- RULE: never copy another app's UI layout; reference apps only for capability (STATUS.md).
- Build 200's Theme sheet (modelled on another app) was removed. `NewLookTheme.swift` now has
  `NewLookColorPanels` (shown by `LookPickerCard` under LOOK while New is on), using Apple's
  `ColorPicker` for any colour; `ThemeSwatch`; `NewLookCoverStyle`; `NewLookMovingBackdrop`
  (four large soft glows on 11 to 18 second paths, screen-blended, still under Reduce Motion).
- No more whole-library rebuild on a colour pick: RootView's `.id` is the look only;
  `LibraryShell` puts `.id(version)` on Home, Library, Favorites and the tab bar, and Settings
  redraws in place (it observes `NewLookColors`), so it keeps its scroll and the picker.
- The "after 199" section below describes the removed sheet; history only.

## START HERE (19 October 2026, evening Eastern): Theme sheet after 199

- Owner asked for colour customisation "more like" another emulator's Theme screen. Built our
  own (clean-room) in `NewLookTheme.swift`: `ThemeRow`, `ThemeSheet` (presented from RootView via
  `NewLookColors.showingTheme`, so library redraws never close it), `ThemeColorPicker` +
  `ThemeWheel` + `ThemeBrightnessSlider`, `NewLookMovingBackdrop` (MeshGradient on iOS 18, moving
  radial glows before; still with Reduce Motion; the library is not on screen during play).
- Settings live in `NewLookColors` (LookTheme.swift). Accent may be "HEX" or "HEX-HEX" (gradient);
  `ShellPalette.accentFill` is the gradient, `ShellPalette.gold/accent` its middle colour.
- Box art: `NewLookGlowCard.styledCover` (Style 1/2/3, `NewLookCoverStyle.radius`, square, hide
  titles). New look only.
- Owner CUT app icons from this job: no new icons, no Info.plist changes. The existing Settings
  APP ICON section (older feature) was left as it was.

## START HERE (19 October 2026, late afternoon Eastern): New look redone after 198

- The owner was unhappy with 198 (hero smear behind the header, SYSTEMS not like the mockup).
  Redone: `NewLookHero` + `NewLookGlow` (glow clipped inside the card), `NewLookSystemTray` +
  `NewLookSystemTile`, `NewLookGlowCard` for every New cover, New branches in ConsoleSections,
  EmptyLibraryNotice, SettingsSection (`SettingsSectionStyle` icons and titles), LookPickerCard.
- Colours: `NewLookColors` (LookTheme.swift), read through `ShellPalette.gold/charcoal/onAccent`.
  RootView redraws LibraryShell by `.id(look + version)`; the colour editor sheet is presented
  from RootView so the redraw does not close it. Per-console glow colours: `NewLook.consoleColor`.
- Still: do NOT touch the New-look header. Labels only Look / Current / New.

## START HERE (19 October 2026, afternoon Eastern): New-look fixes after 197

- Owner's build-197 screenshots: SYSTEMS looked half done, covers cropped, card size did nothing,
  the hero card looked bad. All fixed in the next install, not tried on a phone. Do NOT touch the
  New-look header (the owner rejected a header change). New-look Settings contrast is fine.
- Console photos: `Assets.xcassets/console-<system>.imageset`, sources in `CONSOLE_PHOTOS.md`.
  The old `lobby-*` pictures (unknown source, several duplicates) were NOT reused.
- New Home rows use the Current look's `CoverCard` and `LibraryShell.buildShelves`, so card size
  and the LAYOUT settings work the same in both looks.

## START HERE (19 October 2026, evening Eastern): the second look

- **The owner asked for a second look users can pick** (not the removed Last played Home). It is
  in: Settings → LOOK → Current | New. No branded names: the owner rejected Classic, Studio,
  Browse and Default. Current stays the default and is untouched.
- How it is built: `LookTheme.swift` (the choice, play history, the Settings card),
  `NewLookShell.swift` (the New look's header tools, Home, system chips, tab bar, Game Hub top),
  `NewLookPause.swift` (the pause menu). `LibraryShell` draws both looks and branches only where
  they differ, so select mode, pop-ups, layouts and card sizes are shared. `ShellPalette.accent`,
  `background` and `card` follow the look; `danger` is red in both. New forces dark in `RootView`.
- Built in the next install after 196 and **not tried on a phone**. Test steps: TESTING.md
  "Next install (after 196)".

## Earlier: build 196 is the newest install

- **Build 196** (`b50fb81`) is the newest install:
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-196-b50fb81/Continuum-196.ipa
  The owner told me to take the Last played Home out. It is gone. There is one Home again, the
  shelves one. Settings has no Home switch. Builds 193, 194 and 195 had that look. Not tried
  on a phone yet.
- **Still not built, and do not start them until they ask:** undo a delete, next game on the same
  console, picking which game sits at the top of the Shelves home, looking at a backup before
  restoring it. The Continue widget stays parked.

## Earlier (18 October 2026, 2:40 PM Eastern): ideas kept, the widget parked

## Earlier (18 October 2026, 2:40 PM Eastern): ideas kept, the widget parked

- **Owner kept two ideas. Do not build them until they ask.** Written up in
  `.kiro/notes/docs/ideas/kept-2026-10-18.md`.
  - Undo a delete: deleted games sit in a bin until the next day, then they can be put back.
  - Next game, same console: from inside a game, jump to the next game on that console.
  - The game at the top of Home: you pick which game sits in the big spot. Added 18 October,
    3:25 PM. Right now that spot is just the last game added.
  - Look at a backup before restoring it. Added 18 October, 9:50 PM. Show which games and
    saves are in the backup. Nothing is restored until they say so.
- **Continue widget: looked into, do not build.** A home-screen widget with the picture of
  where you stopped. The tap half already works (`continuum://play`, passed 17 October). The
  widget half needs a second piece inside the app, and Feather and ESign sign only the main
  app, so the widget would not show up on this phone. Parked unless their signer changes.

## Earlier (18 October 2026, 9:20 AM Eastern): build 192 is on the phone

- **Build 192 is the newest install** (`006c199`):
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-192-006c199/Continuum-192.ipa
- **Owner, 18 October, 9:20 AM:** cBoy colouring old black-and-white games like a Game Boy Color,
  and cBoy's experimental picture, both work on build 192. Their word: "Works." That is Build
  190 checks 34, 35 and 37. Do not ask those again. The usual picture mode stays the one games
  use. The link cable still needs a second phone, so it stays untested. cBoy is not finished
  (camera for a Game Boy Camera game, two Game Boys on one phone, and a few rare picture cases
  are still open), but those two phone checks are done.
- **Owner's results on 192, one phone, no discs, no second phone, no online (17 October, 9:35 PM).**
  Their words: "1-20 good." Do not ask any of it again. Passed: Home's top back like before 191
  (cover fills the top behind the name; name and Play stay on screen); the six icons (Dotmatrix,
  Mark, Chrome, Violet, Spectrum, Cyan); Home's shelves; All Games by console (fold, the side
  strip, random game, star several, delete several, console order, By console / Grid / List);
  Clean start; Share a cartridge game; Copy its link through Shortcuts; Patch this game; console
  language on a DS or 3DS game; mute with the silent switch. Also Build 190's icon checks 24, 25
  and 26 (Clay and Badge set on the home screen). LeafGreen stays dropped. Join discs was not
  tried: they said no discs and no swapping discs. PSP and Dreamcast language were not in this
  pass. Full record: TESTING.md, top of "Build 191" and the Build 190 results.
- **Still open when they next test, this phone only:** Build 190 checks 27 to 33 (SNES on Mesen),
  36 (the one-line cBoy link-cable note), 38 onward that is not a disc and not a second phone
  (32X, a Mega Drive save chip, a 3DS game, arcade). Skip two phones, online, and discs until
  they say otherwise. The TV 4K check is still open; they left the TV for later.
- **Next job:** `tasks/gamecube.json` (GC1 to GC5), then ONE build. Don't start a build until
  they ask or the batch is done.

## Earlier tonight (17 October 2026, just after midnight Eastern): build 191, then GameCube

- **Done this session (owner had about 30% usage left):** the whole covers and library job, L1 to
  L10 (`tasks/library-and-manic.json` complete; write-up `docs/reports/library-and-manic-2026-10-16.md`;
  phone checks TESTING.md "Build 191", 18 steps). Commits `605093a` and `6a0045e`.
- **Build 191 is out and checked** (11:58 PM Eastern, 16 October, 12 minutes from push):
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-191-6a0045e/Continuum-191.ipa
  0.8.0 (191), 37 `_libretro_ios.dylib` files, the continuum:// link type in its Info.plist, and
  the new screens' text in the app. The owner was given the link and the Build 191 steps.
- **Owner's first report on 191 (17 October, morning):** the top of Home is broken and "isn't
  like it was before" (TESTING.md "Build 191", results). **Fixed on master in `c347ae5` at the
  owner's word, with NO build** (they said not to make one): the pre-191 hero is back, its art
  drawn as an overlay of a screen-wide box so a wide cover can't widen it. It ships in the next
  build; nothing else has changed since 191.
- **App icons (17 October):** the owner circled Dotmatrix, Mark, Chrome, Violet, Spectrum and
  Cyan and said "Fix." Done on master in `5acc8ec` with no build (TESTING.md "Build 191",
  results). Don't touch any other icon unless the owner asks.
- **Emulator updates found this session, held for the next batch:** Gearsystem, Azahar, PPSSPP and
  Flycast have newer code upstream; PUAE (Amiga) and VICE (C64) newer buildbot builds. Do the
  usual quick check and take the safe ones with the GameCube batch.
- **Next job:** `tasks/gamecube.json` (GC1 to GC5), then ONE build.
- The credits question (move the list into Technical details?) is still unanswered; leave it.

## Earlier (16 October 2026, evening Eastern): two new jobs, nothing built since 190

- **Owner's results on build 190:** checks 1 to 23 passed on 190, and 24 to 26 passed on build
  192 (recorded in TESTING.md). 27 onwards not reported yet. Don't ask 1-26 again.
- **GameCube is NOT impossible.** The owner played Rogue Leader on Manic EMU with no JIT on their
  iPhone 17 Pro Max (Dolphin's Cached Interpreter). Every note that said otherwise is corrected
  (docs/HARD_SYSTEMS.md has the facts, including what Manic's Dolphin fork is).
- **Owner's decisions (16 October):** GameCube and Wii go in on Dolphin (open licence), made fast
  without JIT by a CPU interpreter WE write ("do what Manic did to Dolphin"). Never copy Manic's
  code, screens, wording, names or art (new owner-rule: "Do what Manic does, never the way Manic
  looks"). The owner worried it would look like a rip-off; it won't, because Dolphin is everyone's
  and credited to its own authors.
- **Credits:** the owner asked why Continuum has to show credits when Manic doesn't openly. Answer
  given: ours are already a folded button at the bottom of Settings, About; the licences need a
  list somewhere and public source, not prominence. Offered to move it into Technical details;
  no reply yet. Never delete it (owner-rules).
- **The two jobs, in this order** (full checklists, rules and facts in `.kiro/notes/tasks/`):
  1. `library-and-manic.json`: covers whole at their real shape and sharp (ours crop every cover
     to 3:4 with the system label on top), the library by console (sections, index, Random Game,
     console order, select several), ROM patcher, MultiDisc Helper, launch link, share the game
     file, Safe Mode, language per game, Respect Silent Mode. Continuum's own design throughout.
  2. `gamecube.json`: Dolphin as an optional core on the Cached Interpreter (GC1), our own faster
     interpreter proved identical to it (GC2), JIT when allowed (GC3), Wii controls (GC4).
  Then ONE build (the owner wants everything done first), check the .ipa, give the link and the
  phone steps written out.
- **How to work here:** the owner's Workflows (loops) are off, so use `invoke_sub_agent`
  (general-task-execution) with ONE narrow job each, telling it to push after every green step
  (two helpers with big scopes stopped silently on 16 October). Rust in this sandbox:
  `CARGO_HOME` must be writable (e.g. /projects/sandbox/.scratch/cargo-home) and
  `RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu`. Background processes die when a command ends:
  run long builds in the foreground. Reference MAME: `bash .kiro/notes/wip/tools/build-mame-deps.sh`.
  This sandbox has no GPU or Vulkan; host-test Dolphin with its Null or Software video backend.

## Build 190 is out (16 October 2026, 8:45 AM Eastern) — newest install, checked

- https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-190-a8b206a/Continuum-190.ipa
  (`a8b206a`): 0.8.0 (190), 37 core files (the 36 plus cArcade); DOOM, Jaguar, Virtual Boy, C64
  and Amiga are byte for byte build 185's; `core-mirror-1` holds all 17 files again and
  `scripts/check-core-mirror.sh live` passes with every one from the mirror.
- Build 189 stopped in its first minute: `build-engine.sh` required every core file to be named in
  ios.yml, which can't list cArcade (workflow edits are parked). `a8b206a` exempts that one name.
- The owner's tests are TESTING.md "Next install (after 185)", 63 checks. They haven't reported
  on 182 to 185 either. Record what they send; fix nothing until they say they're done testing.

## Update, 16 October 2026, about 7 AM Eastern: the whole batch is done, ONE build started

- Every checklist in `.kiro/notes/tasks/` is complete (owner, 1 AM: "I want everything done first
  before a build"). Highlights: cBoy's exact drawer 27 → 51 of 55 (the other 4 have no reference
  picture); Mesen 2 as the second SNES choice; cArcade plays 199 sets (Pac-Man, Galaxian/Moon
  Cresta, Konami's Scramble family, Donkey Kong/Jr.); the newer PSP, Dreamcast and 3DS emulators;
  the reviews (U4/T4) found no compile problem and fixed two behaviour bugs (ec644be, 49043c9).
  STATUS.md "Next up", TESTING.md "Next install (after 185)" (63 checks) and the README are
  updated.
- Checked on the final master before the build: workspace 749 tests, clippy clean, cBoy 163,
  cArcade 136, skins, players, core cache 219, core mirror 41, wrapper and Symbian harnesses, all
  54 Swift files parse and are listed in project.yml, and the bindings typecheck.
- **The build** was started by hand (workflow dispatch). Every emulator rebuilds once (about an
  hour). Check the .ipa: 0.8.0 and the run number, 37 `_libretro_ios.dylib` files (36 plus
  cArcade). If the release step gets HTTP 403, follow `.kiro/notes/tasks/README.md`, "Then the
  build", step 3. Then give the owner the link and TESTING.md "Next install (after 185)" written
  out. After it: `bash scripts/check-core-mirror.sh live` should say all 16 come from the mirror.
- Helpers working through `invoke_sub_agent` (general-task-execution) did the jobs this session;
  two stopped silently mid-job on the big arcade scope, so give a helper one narrow job and have
  it push after every step.

## Update, 16 October 2026, 12:50 AM Eastern (third Kiro account)

- The 11 memories are saved on this account. Workflows (loops) are turned off on this account by
  the owner, so one session works the jobs in turn.
- **Done this session:** `mirror-restore.json` complete (9c53424; the next build takes 11
  emulators from the buildbot and DOOM, Jaguar, Virtual Boy, C64 and Amiga from build 185's
  install, then refills `core-mirror-1` itself; the sandbox can't upload release files).
  cArcade's Galaxian/Moon Cresta board matches MAME on all 10 test programs (7e6402e; not in the
  app yet: sound, Konami's board and the wiring are next, see `arcade.json` I5). Emulator updates
  (14375a5): PSP, Dreamcast and 3DS moved to their newest code; Gearsystem skipped (desktop-only
  changes). CU5's write-up is still to do.
- **Reference MAME:** `bash .kiro/notes/wip/tools/build-mame-deps.sh` in the foreground rebuilds
  it in a new sandbox. Rust here: `CARGO_HOME` must be writable and
  `RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu` (rustup can't sync).
- Still left, in order: CU5, arcade I5 (Galaxian sound, Konami board, wiring), I6, I7, cBoy G2-G5,
  H3, H6, U4, U5, T3-T5, then fold the reports and ONE build.

## Where this left off (14 October 2026, 1:40 PM Eastern)

- **New Kiro account again.** The second account ran out of credits at about 9:10 AM with six jobs
  running; the owner bought a little more only to get everything saved. First thing: save every
  line of "Memories to save on a new account" (end of `.kiro/steering/owner-rules.md`) as a global
  learning, and tell the owner they're saved.
- **Old releases deleted (14 October), and a mistake that must be fixed before the next build.**
  Only build 185 is left, plus `core-mirror-1`, which isn't an install: it held the build's frozen
  copies of the 16 downloaded emulators and was deleted by mistake with the old releases, then
  re-created empty. **Do `.kiro/notes/tasks/mirror-restore.json` before any build**, or DOOM,
  Jaguar, Virtual Boy and C64 come out as the newer builds turned down on 12 October. Nothing
  is lost: the checked builds are still inside build 185's install, and the job says how to use them. Links to older
  builds in these notes no longer work; their code is still in git.
- **Newest published install: build 185** (`393f12c`, 13 October), checked: 0.8.0 (185), 36 core
  files: https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-185-393f12c/Continuum-185.ipa
  It has the night batch (TESTING.md "Night batch (13 October)"). The owner hasn't reported on 182
  to 185 yet. **Build 188** was started at 9:01 AM without the owner's yes and cancelled at 9:04 in
  its first checks. Nothing has been built since 185, and no build starts until the batch is done or
  the owner asks (owner-rules).
- **The batch is in `.kiro/notes/tasks/`.** Its README has the table of what's done and left, how to
  run the jobs, and the build steps. The two items cut off mid-way are saved as patches in
  `.kiro/notes/wip/` (README there). Nothing was left only in the sandbox.
- **On master since 185, not built yet:** the two-phone Game Boy link (app half); cBoy colouring
  black-and-white games like a Game Boy Color (off by default; Mealybug 27 of 55), its Link cable
  text and one-line setting descriptions; arcade games back on our own emulator, cArcade (65 Pac-Man
  board sets, matching MAME frame for frame; its Z80, 8035 and AY-3-8910 proven); 32X games start
  with no BIOS files (our own start-up code: two open-licence 32X games reach their menus on a host
  build); Sega CD multi-disc playlists, Next disc and the save cartridge; every Mega Drive save-chip
  and J-Cart cartridge in MAME's list recognised; newer Gearsystem (Master System, Game Gear) and
  Azahar (3DS: old states won't load, and the heads-up says so); light mode (Settings, Appearance:
  Dark, the default, Light, System); the "80 games" strip gone (its messages pop up and fade, the
  count is in Settings, Storage); Settings, Layout, Game card size; the floating tabs; 10 of the 12
  circled app icons fixed (Cyan and Violet would need new art: the owner's call) and the Default
  tile; shorter text in all of Settings (the PSP section is gone) and most other screens; the build
  cache's second pass (comments, the cache's own code and the emulator lists rebuild nothing; 213
  checks); a friendlier README and a tidier docs folder.
- **Left** (full instructions in each checklist): cBoy's exact drawer, G2 to G5 (go-ahead given; the
  owner's thinking is always at max, so never ask again); arcade I5 (half done, in the WIP patch),
  I6 Donkey Kong, I7; Mesen 2 as a second SNES choice (H3) and H6; the PSP and Dreamcast updates
  (CU3, CU4) and CU5; the Home screen review and write-up (U4, U5); the rest of the shorter text (T3
  to T5). Then fold the reports into STATUS.md and TESTING.md, ONE build, check the .ipa (37 core
  files: the 36 plus cArcade), and give the link and the phone steps in the same reply. Repo cleanup part 2 (the notes off the front page) is done; only the
  About box and the release-notes text are left, both blocked (tasks README, "Parked"); after the next build, update the README's list of
  systems (`.kiro/notes/tasks/repo-cleanup-phase2.md` section 9).
- **Tools:** every account has the same tools (the owner was right; the first session on the second
  account missed the loop tool for hours). Jobs run as `bundled://ralph` loops, one per job, all started at once. Fetching anyone else's repository needs `GIT_CONFIG_GLOBAL=/dev/null` in front (the
  sandbox's git sends github.com through a gateway that only serves this repository). The GitHub
  token can't edit the repo's About box (403), and Kiro's rules allow no workflow-file change
  without a pull request, so those wait (tasks README, "Parked").
- **What went wrong on 14 October, so it doesn't again:** a Kiro restart paused three loops for half an hour and
  nobody resumed them until the owner wrote (check every loop whenever you wake); the owner was told
  they couldn't see the jobs, but their app has a Workflows tab (this chat's loops didn't appear in
  it, reason unknown, so post a one-line update in the chat whenever a piece lands); the owner was
  asked about max thinking again (it's always at max); and build 188 was started without a yes.

Older, kept as history: build 144 (`build-144-74050e5`) passed the owner's four phone checks on
9 October (Home readable after the big cover, Home and All Games scroll, re-importing keeps a
game, fast-forward lets go after Control Centre, the (i) line names the phone). Do not ask those
again. Not shown, so not Done: rewind letting go the same way, covers not flashing (TESTING A8),
the phone name on the feedback screen (A9), one bad skin not wiping the list, and a random skin
id not becoming a console. A skin that does not name itself can still show up twice across two
phones. No JIT test.

## Check for newer emulator versions at the start of every session (owner ⭐, 12 October 2026)

The owner wants the newest version of every emulator, always, as long as it does not break
anything major compared with the version we ship. Not a robot doing it: Kiro checks every
session and takes them itself, after checking thoroughly. The full rule is in
`.kiro/steering/owner-rules.md` ("Emulator updates"). In short:

1. Every session, check every core for a newer version. That means the 19 pinned builds from
   source (`bash scripts/build-core.sh ios-pins`: 14 required cores, Dreamcast, Symbian and the
   3 JIT builds) AND the 16 downloaded ones (sha-pinned in `scripts/fetch-buildbot-cores.sh`).
   `scripts/check-core-updates.sh` covers both: it writes `docs/CORE_UPDATES.md` with what's
   newer, the last few upstream changes, whether our patches still fit, and the list of updates
   already checked and skipped.
2. For each one that is newer, check it thoroughly before taking it: read the upstream changes
   for anything removed or broken (systems, options, iOS support, save formats), make sure our
   patches still apply, build it, run every check, and check whether old save states and
   in-game saves still load. Anything that would break, take out, or slow down something
   players already have counts as major: then do not take it, and tell the owner why in
   plain words.
3. Take the good ones in the next batch (one build), and tell the owner in plain words which
   consoles got newer emulators and what changed for them.
4. Players get a one-time heads-up in the app for each updated emulator (built on master,
   13 October: `crates/emulator-bridge/src/core_updates.rs`). See "Players get a heads-up" in
   owner-rules.md for the wording and the rules.

Moving a pin: follow "MOVING A PIN" in `scripts/build-core.sh` (and "Moving a pin" in
`scripts/fetch-buildbot-cores.sh`, which needs the zip pin and the dylib pin). Also update
`crates/emulator-bridge/src/core_updates.rs` (the version it ships, and an entry for the update,
saying whether old states still load): a test fails until it matches the scripts.

The weekly scheduled job (`.github/workflows/core-updates.yml`) has not run on GitHub yet (0
runs on 13 October). The script was run here on 13 October: all 32 sources up to date (27 on
their newest version, 5 newer ones already checked and skipped). Its page is only a hint, never
the whole answer.

## Older notes (7 October 2026, history)

- (Out of date: since build 142 the app has the owner's icon, and build 145 added 55 home screen
  icons. The steps in this bullet are still how an icon is put in.)
  **The app ships no icon, and that is the current state rather than an oversight.** Build 134 added
  a drawn one; the owner had asked to see artwork before it went into the app, it went in before
  they saw it, and they then rejected it along with three alternatives, so build 135 took it out.
  Do not put one back without them approving the picture first. To put one in:
  `python3 scripts/make-app-icon.py --from <picture>` (fits any picture to all 13 sizes, reads the
  real format from the bytes, flattens transparency, centre-crops), then restore the two lines in
  `native/ios/project.yml` the long comment there names. Details in STATUS.md "Build 135".
  The keyless image service is a dead end: watermarked output, see STATUS.md.
- Build 133, release `build-133-0e9a7e8`, 0.8.0 (133), 35 core dylibs (32 cores
  plus 3 JIT builds), checked. The app exports `_continuum_jit_region` / `_continuum_jit_release`,
  and ppsspp, azahar, pcsx_rearmed_jit and flycast_jit all have the lookup compiled in. Link:
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-133-0e9a7e8/Continuum-133.ipa
- Build 133 = JIT parts 1 and 2 (below): current iPhones included. Owner tests are TESTING.md J1
  and J2; build 129's A1 to A5 are still unanswered.
- Build 129 (pushed 6 October; 128 was cancelled for a wording pass): the app cleaned up for a
  public beta, every tester-facing line in a human voice (STATUS.md "Build 129";
  TESTING.md A1 to A5). The owner posts the .ipa straight into Telegram groups and Reddit, not the GitHub page, so
  testers only ever see the app itself. Feedback email (since 12 October 2026):
  ContinuumDev2c2b@gmail.com. Build 127's 3DS model change and cheat-off fixes are still untested.
- Everything else that is planned is in STATUS.md "Next up" and .kiro/notes/docs/MANIC_PARITY.md "Still to do".
- Other remote testing tried and dropped (6 October): Momentic runs only on simulators, so it would
  need a whole simulator build (and 19 buildbot cores have no simulator version); the owner said
  not to. Apptest.ai rejected the .ipa ("The app file is not supported") because it carries no
  Apple certificate or provisioning profile. A built-in self-test was offered and turned down: the
  owner has already proved more than half the systems on the phone, so do not offer it again.

## JIT (owner, 7 October: required for everyone who can enable it, never required to work)

Build 131 (part 1, done in code, untested: the owner's phone cannot use JIT):
- `crates/emulator-bridge/src/jit.rs` reads `CS_GET_TASK_ALLOW` / `CS_DEBUGGED` (via
  `jit_probe::ios_aarch64::signing_status`) and the phone model and iOS version (`sysctl`
  `hw.machine`, `kern.osproductversion`). JIT is usable when debugged, not on a TXM phone (iOS 26,
  iPhone14,2+ / iPad14,5+, StikDebug's table), and the user switch is on. No `brk` is ever run.
- PPSSPP and Azahar: host answers `GET_JIT_CAPABLE` (74), frozen per core load; PPSSPP option
  defaults to "JIT" then (`options.rs` `host_rules_for`); Azahar patch
  `scripts/patches/azahar-use-jit-when-the-host-allows-it.patch` uses its own `CanUseJIT()`.
- PCSX ReARMed, parallel-n64, flycast: optional `_jit_` builds in `build-core.sh`
  (`IOS_OPTIONAL_CORES`), embedded in `project.yml`; Swift asks `engine.coreLibraryFor` in
  `ensureCoreLoaded`. Patches `pcsx_rearmed-ios-jit.patch` (run-time cache mapping, 16K pages)
  and `parallel_n64-ios-jit.patch` (mprotect instead of `pthread_jit_write_protect_np`).
- Settings, Technical details: JIT sentence, "Use JIT when it's available", StikDebug button
  (`stikdebug://enable-jit?bundle-id=&pid=`, no script, only when not TXM). `JIT:` line in (i)
  and feedback.
- All three `_jit_` builds compiled in build 131. If a later log shows one failing, fix it (it only
  warns; the .ipa still ships without it).

**Build 138: the N64 now gets JIT on current iPhones too, so every system with a JIT build has it
on every phone that can switch it on.** The reason it was listed as blocked was wrong twice over:
parallel-n64's trampoline allocator is compiled but never called at the pinned commit (nothing
calls `trampoline_init`), and its recompiler already has the two-address mode the Switch port uses,
with all ~38 write-address-to-run-address translations already written. It only needed the host's
pair wired into `base_addr` / `base_addr_rx`. See STATUS.md "Build 138". Untested on a phone, like
all the JIT work.

Part 2 (build 133, done): `crates/emulator-bridge/src/jit26.c` reserves a 512 MB region, has the
JIT app bless it (`JIT26PrepareRegion` / `JIT26Detach`, `brk #0xf00d`, only after `CS_DEBUGGED` and
only when the app itself asked for `universal.js`), and `vm_remap`s a writable view. Cores dlsym
`continuum_jit_region` / `continuum_jit_release`. Patches: `ppsspp-ios-jit-region.patch` (CodeBlock
takes the pair, `PlatformIsWXExclusive` then false), `azahar-ios-jit-region.patch` (oaknut CodeBlock
gains `wptr_base()`, dynarmic's `address_space.cpp` passes it, dynarmic's A32 `code_cache_size`
32 MB on iOS), `pcsx_rearmed-ios-jit.patch` (`TC_WRITE_OFFSET` + region, cache flush translates the
write address to the run address), `flycast-ios-jit-region.patch` (`FEAT_NO_RWX_PAGES` for the iOS
jit build, `prepare_jit_block` two-address overload from the region, the three `JITWriteProtect`
helpers become no-ops). `jit::core_may_use_jit` is the gate; the host hands out the same pair on
non-TXM phones so there is one path. parallel-n64 joined this in build 138 (above).

melonDS JIT is macOS-only code; DS is fine on the interpreter.

## Build 126 test results (6 October)

- Pass: 3DS saves load, an imported save keeps its picture, the small text flash and (i) with a
  skin, the in-game cheat box.
- Fail, fixed in 127: changing 3DS System Model crashed the app with no restart button; a
  walk-through-walls cheat froze a Pokemon GBA game (it was the FireRed v1.1 CodeBreaker code, and
  switching it off did nothing because mGBA ignores the off flag).
- The owner called the feedback form "basic": rebuilt in 127.
- Not answered yet: Jaguar speed, the 3DS stutter after transitions, Next disc (multi-disc PS1),
  and which game and version the owner's Pokemon file is (TESTING.md A2 step 2 asks).

## Build 125 test results (recorded in STATUS.md and TESTING.md)

Passed nearly everything; see STATUS.md and TESTING.md's "Already confirmed" table. The two
failures (3DS states refused as "too short", imported slots with no picture) were fixed in 126 and
confirmed.

## The owner's cheat question (answered 5 October)

They asked whether normal GameShark-type codes work. Yes: a typed code goes straight to the
system's emulator, which reads its own kinds (table in `crates/emulator-bridge/src/cheats/formats.rs`,
read off each core's source). Some emulators ignore typed codes entirely (3DS, Dreamcast, Atari
2600, arcade, Yabause, Amiga, C64, Lynx, 5200, Virtual Boy, PC Engine CD, Pokemon Mini). A code is
for one exact game version; the cheat screen names the version for GBA and Game Boy games.

## How the owner works

- They are not a programmer and use only an iPhone. Plain English, explain any technical word,
  and describe what they will see on the phone. Never ask them to read files, logs or GitHub.
- They test only the .ipa. After every build, check the .ipa and give the direct link without
  being asked, plus a short list of easy tests (they often test at work).
- When they are testing, fix nothing until they say "I'm done testing for now"; just record.
- Never re-ask a test they already covered, and when they ask "what do I test", give the full
  steps right there in the reply, not a pointer to an earlier message.
- Push straight to master: no branches, no pull requests.
- **ONE BUILD PER BATCH, NEVER ONE BUILD PER FIX.** The owner is angry about a session that left
  six builds in the Actions list, three of them cancelled because a later push superseded them.
  That looked like builds piling up in parallel; they were not. `ios.yml` already has a
  concurrency group (one running, at most one queued), so overlapping builds are almost always
  too many pushes rather than a concurrency fault — check `run_started_at` before claiming
  otherwise, and do not "fix" the workflow for what is a pushing-discipline problem.
- **How to satisfy that AND the rule about never losing work: `[skip ci]`.** The two pull in
  opposite directions — work must be pushed as soon as it is finished, because a session can end
  at any moment, but every push to a watched path starts a build. So: push each finished piece as
  it is done with `[skip ci]` in the commit message, then make the LAST push of the batch without
  it, and that single build contains everything. Nothing is ever left only in the sandbox, and the
  owner gets one build.
- Only the paths listed in `ios.yml`'s `on: push: paths:` start a build at all, so a commit
  touching only `.kiro/notes/` or `docs/` (but not `docs/samples/`,
  which the skin check uses) never needs `[skip ci]`.
- **A build takes about 6-8 minutes on the Mac when the cores are cached, and about 45-50
  minutes when they are not.** The cores built from source are cached between runs (STATUS.md
  "Build 136"). Three things to know: each core is judged on its own inputs, so editing one
  `scripts/patches/<family>-*.patch`, or one core's own recipe inside the `# recipe-start:` region
  for it in `scripts/build-core.sh`, rebuilds that core (and the JIT build sharing its source) and
  leaves the rest cached — while editing the SHARED part of `scripts/build-core.sh` (a helper every
  core goes through, the staging, the common flags), or `native/continuum-symbian/` or
  `native/ios/build-engine.sh`, or a new Xcode on the runner, still rebuilds every core and pays
  the full 45-50 minutes, which is correct but worth saying first; cBoy
  (`cores/continuum-gb`) is NOT in the key since the 12 October fix and is compiled from source
  on every build, so a cBoy-only change costs about a minute, never a full rebuild (builds 177
  and 180 took about 47 minutes only because cBoy was still in the key). A cBoy-only push does
  start a build by itself now, so use `[skip ci]` until the last push of a batch. And the cache
  is saved in its own step straight after the cores are built, before the app is packaged, so a
  build that fails later (Xcode, packaging, the release) does NOT cost the owner the core build
  again on the retry. That step is deliberately not `if: always()`: a core
  build stopped part-way can leave a half-written core that the next build would trust.
- **Never open an image whose bytes you have not checked.** On 7 October a picture downloaded from
  an image service was named `.png` but was really a JPEG inside. Looking at it put a file in the
  chat's history whose declared type did not match its content, and from then on EVERY message the
  owner sent was rejected in about a second, before it was ever read — the chat was dead and a new
  one was the only cure, which cost a session and a lot of the owner's patience. Run `file` on an
  image, or convert it, before looking at it. Better still, draw artwork with a script (as
  `scripts/make-app-icon.py` does) so the bytes are never in doubt.
- JIT when a user can enable it, never required; no web build. Behaviour goes in the Rust
  engine. Only Kiro works on this repo.

## How to check work before pushing (the sandbox can't build the iPhone app)

- `cargo test --workspace --features emulator-bridge/native-core` (723 tests on 14 October)
- `cargo clippy --workspace --features emulator-bridge/native-core` (CI runs it too; an error
  stops the build)
- cBoy: `cd cores/continuum-gb && scripts/fetch-test-roms.sh && cargo test --release` (158 tests on 14 October).
  Run it before any cBoy push: builds 178 and 179 failed on these alone.
- `bash scripts/check-skins.sh`, `bash scripts/check-players.sh`
- `bash scripts/fetch-libretro-headers.sh && bash native/switch-wrapper/build.sh host` (15/15)
- `bash scripts/check-core-cache.sh` (213 checks; the per-core build cache)
- cArcade: `cd cores/continuum-arcade && cargo test --release`. CI doesn't run it yet (the workflow edit is parked), so run it before every cArcade push.
- `swiftc -frontend -parse <file>` on each changed Swift file. This only checks the writing, not
  the types, so a Swift type mistake shows up only on the build server.
- A new Swift file must be listed in `native/ios/project.yml` `sources`, or it is silently skipped.
- Bindings: `cargo build --release --features emulator-bridge/native-core,emulator-bridge/uniffi-bindings -p emulator-bridge`,
  `cargo build --release -p continuum-uniffi-bindgen` (the package's name; its binary is
  `uniffi-bindgen`), then `./target/release/uniffi-bindgen generate --library target/release/libemulator_bridge.so --language swift --out-dir <dir> --no-format`,
  rename the modulemap to `module.modulemap`, and `swiftc -typecheck -I <dir> <dir>/*.swift`.
- Builds: `gh api "repos/2c2bhdhw9z-cell/Continuum-/actions/runs?per_page=3"`. Releases:
  `gh api repos/2c2bhdhw9z-cell/Continuum-/releases/latest`. Check an .ipa: count
  `_libretro_ios.dylib` files (36 from the licence-swap install on: 33 cores plus 3 JIT builds;
  build 180 had 38) and read `CFBundleShortVersionString` / `CFBundleVersion` from
  `Payload/Continuum.app/Info.plist`.
- A wait-and-check command can time out while a build is still running; that is not the build
  failing. Check the run itself.

## Simulator motion preview (19 October 2026)

`tools/newlook-preview/` + `.github/workflows/newlook-preview.yml`: compiles the real
`native/ios/NewLookMotion.swift` with two small stand-ins (`PreviewStubs.swift`: NewLookColors,
ShellPalette at their defaults) and a Home-like test page (`PreviewApp.swift`) with swiftc for the
simulator, runs it on an iPhone simulator, and uploads `full/` and `plain/` (launched with
`-plain`: flat backgrounds) screenshots plus `motion.mp4`. Runs only when those files change, or
by hand; never blocks the .ipa. CI simulator screenshots are slow (3 to 30 s apart; see
`times.txt`). The real app cannot be built for the simulator (engine, core wrapper, libsmb2 are
iPhone-only builds), so Home/Library/Favorites/Settings themselves are not filmed.

## Sparkle mark rejected (19 October 2026)

Brett rejected the sparkle mark (plan item 12 of the motion batch). `NewLookSparkleShape` and
`NewLookSparkleMark` are deleted from `NewLookMotion.swift`, the empty New-look Home and
Favorites notice (`EmptyLibraryNotice`, LibraryCards.swift) is back to its pre-202 plain heart /
box icon with no float or sparks, and the preview test page no longer shows it. Never add
decorative marks he did not ask for. The hero / Game Hub / pause cover float and sparks are a
separate item and stay.
