# Hard systems: GameCube, Wii, Symbian/N-Gage, Dreamcast

Plain answers for the owner. The rule that decides all of this is in
[PLATFORM_LIMITS.md](PLATFORM_LIMITS.md): this phone has a distribution-signed install, no debugger
and no computer ever, so **no JIT**. Every system here has to run on an interpreter.

Speed reference: the iPhone 17 Pro Max's A19 Pro scores about 3,895 single-core in Geekbench 6
([TechSpot](https://www.techspot.com/news/109422-apple-a19-pro-single-core-benchmarks-beat-snapdragon.html)).
The iPhone 15 Pro Max (A17 Pro) scored about 2,914
([TechSpot](https://www.techspot.com/news/100160-apple-a17-pro-soc-single-core-benchmark-close.html)).
So this phone is roughly a third faster per core than the 15 Pro Max. Emulators lean on one core,
so that is the number that matters.

## What Manic EMU says about JIT

Manic's own README says compatibility and speed depend on "JIT availability", and that App Store
and sideloaded builds differ there ([README](https://github.com/Manic-EMU/ManicEMU/blob/main/README.md)).
Its changelog lists GameCube/Wii (Dolphin) and Symbian (EKA2L1) with "Legacy / PPL / TXM JIT", warns
that GameCube and Wii still bring heat and frame drops, and in 2.0.1 adds a "built-in,
self-contained JIT, no external debugger required" ([changelog](https://manicemu.site/changelog/)).
Manic's repo includes StikJIT as a submodule. StikJIT's own guide says the app getting JIT must
have `get-task-allow` and that a pairing file must be imported
([INTEGRATION.md](https://github.com/StikDebug/StikJIT/blob/main/INTEGRATION.md)). So
"self-contained" means the helper is built into the app. It still needs a development-signed
install and a pairing file made with a computer. **Neither is possible for this owner.** Manic's
GameCube, Wii and Symbian speed comes from JIT, and Continuum cannot copy that.

The libretro iOS buildbot has `dolphin_libretro`, `flycast_libretro`, `ppsspp_libretro` and
`azahar_libretro` (checked 3 and again 5 October 2026). PLATFORM_LIMITS.md now says so too.

---

## GameCube and Wii (Dolphin)

- **Core or iOS port:** yes, both. libretro Dolphin is active and the buildbot ships an iOS arm64
  build. DolphiniOS and Provenance's iCube are standalone iOS ports.
- **Builds for iOS arm64:** yes. I downloaded the buildbot dylib. It contains the ARM64 JIT, the
  "Cached Interpreter" and a Vulkan back end with libretro Vulkan glue (`DolphinLibretro/Vulkan.cpp`).
  Continuum already runs Vulkan cores (Azahar, PPSSPP).
- **Without JIT:** it runs, but it does not play. Dolphin's team wrote that the Cached Interpreter,
  even after its 2024 "2.0" rewrite, is 3 to 4 times faster than their plain interpreter but still
  "cannot serve as a JIT alternative", that the JITs are "MUCH faster in every scenario", and that
  no big gain is left
  ([Dolphin progress report 2407/2409](https://dolphin-emu.org/blog/2024/09/04/dolphin-progress-report-release-2407-2409/)).
  The original Cached Interpreter was described there as "an order of magnitude slower" than the
  JIT. DolphiniOS's developer showed the interpreter on an iPhone 15 Pro Max and called it "basically
  unplayable" ([OatmealDome](https://oatmealdome.me/blog/why-dolphin-isnt-coming-to-the-app-store/)).
  DolphiniOS's help page also says Dolphin is "unplayably slow" without JIT
  ([JIT help](https://dolphinios.oatmealdome.me/jit-help)).
- **Expected speed here:** my estimate, not a measurement: a third faster than that 15 Pro Max
  test, so roughly a tenth to a fifth of full speed in 3D games. A few very light games, and menus,
  may move. Real games will not be playable.
- **What it would take:** fetch the buildbot dylib (or build it in CI), force the CPU option
  `dolphin_cpu_core` to the Cached Interpreter in the engine's option rules (the JIT setting would
  try to run generated code and iOS would kill the app), route the extensions, add the GameCube and
  Wii pads. A few days of work.
- **Recommendation:** **do not ship it as a playable system.** If it goes in, label it plainly as
  "experimental, very slow, needs JIT to be playable". It cannot be made playable on this phone
  without JIT. That is a hard limit, not missing work.

## Symbian / N-Gage (EKA2L1)

- **Core or iOS port:** no libretro core. EKA2L1 itself has an iOS build script in its own repo
  (`scripts/build_ios.sh`), which turns its JIT (dynarmic) off for signed builds and then runs on
  "dyncom", an ARM interpreter that came from Citra.
- **Builds for iOS arm64:** EKA2L1's own app does. As a Continuum core it does not exist: someone
  has to write a libretro wrapper around EKA2L1 (boot, firmware install of `.ROM`/`.RPKG`, `.sis`
  installs, OpenGL ES output, input, saving).
- **Without JIT:** possible, with limits. The guest phones are slow ARM chips (N-Gage about
  104 MHz, S60v3 phones a few hundred MHz, Symbian^3 up to about 1 GHz). The best evidence on this
  very phone: the 3DS core (Azahar), also with its JIT compiled out, runs Mario Kart 7 here with
  stutter (STATUS.md). The 3DS's ARM11 is 268 MHz. So older Symbian and N-Gage games are likely to
  be playable on the interpreter. S60v5 and Symbian^3 games (faster phones) likely are not. That is
  my reasoning from those facts, not a measurement; EKA2L1 publishes no interpreter speed figures.
- **What it would take:** the wrapper above is the whole job, probably one to two weeks of
  focused work, plus CI build time. EKA2L1 is big C++ with many submodules.
- **Recommendation:** **worth doing later, after Dreamcast is proven.** Promise N-Gage and early
  S60 games only.

## Sega Dreamcast (flycast)

- **Core or iOS port:** yes. Upstream flycast is a libretro core. The buildbot's iOS flycast
  contains the ARM64 dynarec and the text "Cannot run without JIT", so it is no use here.
  Continuum already builds flycast from source in CI with `TARGET_NO_REC`, which compiles the
  dynarec out and leaves only interpreters (SH4, ARM7 sound CPU, DSP). This is the right build.
- **Builds for iOS arm64:** yes. It builds green in CI and `flycast_libretro_ios.dylib` has been
  in every IPA since build 119 (checked in the build 124 file). **Nobody has played a Dreamcast
  game on the phone yet**: that is test C19 in TESTING.md.
- **Without JIT:** unknown on this phone; it has to be measured. Stock flycast's interpreter is
  slow: a browser port reported "a couple of FPS" on the interpreter
  ([flycast-wasm](https://github.com/nasomers/flycast-wasm)), though a browser is much slower than
  native code. Against that, Provenance's iFly claims full-speed Dreamcast on iPhone with **no
  JIT** ([iFly](https://ifly-emu.com/)). Its flycast fork is open source and has public JIT-less
  interpreter branches (`JoeMatt/JitLessIR`, `JitLessIR2`, mid-2025) in
  [Provenance-Emu/flycast](https://github.com/Provenance-Emu/flycast). I have not verified the
  claim.
- **Expected speed here:** my estimate: stock interpreter, light and 2D games may be near full
  speed and heavy 3D games well below it. With iFly's interpreter work, possibly full speed.
- **What it would take:** first, a phone test of the current build (one game, look at the fps;
  TESTING.md C19). If it is too slow, try building from Provenance's JIT-less branch instead (GPL, same libretro
  target), which is a CI change, not new code.
- **Recommendation:** **the best of the three. Keep it, test it, and if slow switch to the
  Provenance interpreter branch.** It is the only one here that can plausibly be fully playable with
  no JIT.

## In one line each

| System | Playable with no JIT on this phone? | Do it? |
| --- | --- | --- |
| Dreamcast | Likely for many games; proof needed | Yes. In the app since build 119; test it now |
| Symbian / N-Gage | Older games likely, newer ones unlikely | Later, needs a wrapper (not started) |
| GameCube / Wii | **No** | Not built. Only as a labelled experiment, or not at all |
