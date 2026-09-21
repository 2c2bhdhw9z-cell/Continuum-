# What is finished, and what is not

One page, kept current, so nothing has to be inferred from a commit log. Three states only:

- **Done** means built, in the `.ipa`, and confirmed working on a device.
- **Built, untested** means it is in the app and should work, and nobody has tried it yet. Those
  are the rows in [TESTING.md](TESTING.md)'s queue.
- **Partial** means some of it works and a named piece is missing. Read the note.

If a row says Partial, the note says exactly what is absent. Nothing here is rounded up.

---

## Systems

**Eight** cores, **fourteen** systems, shipping today. Two of those systems cost no new emulator at all, both having been supported already by cores in the app and simply not wired up. Four systems are still ahead and none of them is in this
table: **N64, PSP, 3DS and the Switch**, hardest last. They all need the hardware-renderer work, so
they live in [the road below](#the-road-to-the-rest-of-the-systems) rather than here.

| System | Core | State | What is missing |
| --- | --- | --- | --- |
| NES | fceumm | **Done** | |
| SNES | snes9x | **Done** | |
| Game Boy | mgba | **Built, untested** | No `.gb` file has ever been imported. GBC and GBA on the same core are confirmed, so this is a formality rather than a doubt |
| Game Boy Color | mgba | **Done** | |
| Game Boy Advance | mgba | **Done** | |
| Master System | genesis_plus_gx | **Built, untested** | No `.sms` file has ever been imported. Game Gear and Mega Drive on the same core are confirmed |
| Game Gear | genesis_plus_gx | **Done** | |
| Mega Drive / Genesis | genesis_plus_gx | **Done** | |
| PlayStation | pcsx_rearmed | **Done** | Interpreter, not the recompiler. Fast enough, and see Recompiler below for why it is not switched on |
| **Famicom Disk System** | fceumm | **Built, untested** | Needs `disksys.rom`, which is Nintendo's own code and cannot ship with the app. The launch path checks for it by name and says so rather than letting the core fail |
| **Sega SG-1000** | genesis_plus_gx | **Built, untested** | Needs nothing extra |
| **TurboGrafx-16** | mednafen_pce_fast | **Built, untested** | HuCard games only. PC Engine CD needs a system card BIOS that cannot ship |
| **Atari 2600** | stella2023 | **Built, untested** | `.a26` only. A 2600 ROM named `.bin` has to be renamed, because `.bin` belongs to the PlayStation here as a disc track |
| **Nintendo DS** | melonDS | **Built, untested** | The core builds, links and ships, and every part of it including the touch screen is now wired end to end. **Nothing has booted on a device yet.** See the DS section below |

### Nintendo DS, in detail

Broken out rather than left as one row, because "untested" hides how much of it is proven:

| Piece | State |
| --- | --- |
| Core compiles and links for iOS arm64 | **Done**, 5.7 MB, and it exports `retro_run` |
| Shipped in the `.ipa` | **Done** |
| `.nds` recognised, imported and routed | **Built, untested** |
| Both screens drawn | **Built, untested**. The framebuffer is 256x384, which is both screens already stacked, so the existing compositor should draw it with no changes |
| Buttons, D-pad, L and R, Select and Start | **Built, untested** |
| **Touch screen, engine side** | **Done**. `RETRO_DEVICE_POINTER` did not exist at all; it is now in the input layer, merged per source, exported as `applyPointer`, and covered by 8 unit tests |
| **Touch screen, app side** | **Built, untested**. The pad owns the stylus, because it already owns touches and already knows where the picture is. The lower half of the picture is the digitiser; a touch is mapped through the letterboxed picture rect, so it lands under the finger rather than off by the thickness of the letterbox |
| **Touch screen, switched on in the core** | **Done**. It was off: the core's touch mode starts at *disabled*, not at the mouse control it advertises, so the screen was dead inside the core regardless of what the app sent. Now answered explicitly, with tests |
| Boots the cartridge rather than the firmware menu | **Done**. The same trap: *boot game directly* advertises enabled and starts off, which would have sent the core to a firmware menu that a generated firmware cannot launch a game from |
| BIOS | **Answered, no files needed**. This build carries a FreeBIOS and generates a firmware when the dumps are absent. The three names stay declared so Settings still reports what it finds, and none is a fine answer |
| Speed | **Unknown**. Software rendered and single threaded: the core has a threaded renderer option that is left off, so if the DS runs slow that is the first lever to pull |

---

## Features

| Feature | State | What is missing |
| --- | --- | --- |
| Import, including multi-file `.cue` plus `.bin` | **Done** | |
| Library, cover art, the detail card | **Done** | |
| Cover art from the internet | **Done** | |
| Cover art from your own file | **Done** | |
| Cover art captured from the running game | **Built, untested** | |
| On-screen controls | **Done** | |
| Physical controllers | **Done** | Including a controller and thumbs at the same time |
| Sound | **Done** | |
| Volume and mute | **Done** | |
| Screen fit and scaling | **Done** | |
| Fast forward | **Done** | Tops out near 4x, which is an engine limit and is stated in the UI |
| Rewind | **Done** | |
| Save states, slots, delete | **Done** | The slot list was reported failing on some games; that was a bug of mine and is fixed but not retested |
| Auto-save and resume | **Done** | |
| Cheats | **Done** | |
| On-screen control layout editor | **Partial** | Reported not working three times. The panel is smaller now so it cannot cover the pad, and it shows a live drag counter to say whether touches are arriving at all. Waiting on that reading |
| Save state compatibility refusal | **Built, cannot be tested deliberately** | Only fires for a state from a different core or build |
| Honouring what a core wants its content as | **Done** | Every core used to be handed a file path and no bytes. That worked for the first six by luck, and would have given Stella a zero-byte ROM, because it copies straight from the data pointer with no path fallback. The engine now reads what each core declares and loads the file when the core wants bytes, so the next such core needs no change |
| File formats per system | **Done** | Every extension is now taken from the cores' own declared lists rather than a hand-written one. That added the two systems above plus `.smd`, `.swc`, `.fig`, `.unf`, `.unif`, `.sgb`, `.mdf` and `.toc`, which were being refused despite being supported |
| Core setting overrides | **Done** | The host refuses a core's requests for its settings, so every core keeps its own defaults. Two DS settings had to be answered because they do not start at the default they advertise; everything else, for every core, is still refused |
| Multi-screen compositor | **Done, unused** | Draws N regions of one texture to N places. Nothing selects more than one yet: it exists for rearranging the DS screens and for hardware-rendered cores |
| Android `.apk` | **Not started** | The one other PLATFORM, and the only one after the iPhone. Next in order, still far off in time. Everything new goes in the Rust engine so Android inherits it |
| Switch wrapper (to EMULATE the Switch) | **Partial** | `native/switch-wrapper/` has the frame gate, a Vulkan stub and a test harness, with no engine behind it. Steps 10 to 12 of the road below |

---

## The road to the rest of the systems

All twelve steps of the sequence in
[docs/SET_HW_RENDER_DESIGN.md](docs/SET_HW_RENDER_DESIGN.md) section 13, not just as far as N64.
Shown whole because stopping the table at step 6 hid the fact that **the Switch is on this list**,
at steps 10 to 12, and that it is a system to be emulated rather than a device to run on.

The DS is not on here at all, and that is why it shipped first: melonDS is software rendered on
iOS, so it needed none of this.

| Step | State |
| --- | --- |
| 1. wgpu owns the one `MTLDevice` | **Done** |
| 2. Composite pass generalised to N screens | **Done**, with 7 layout tests and 3 shader tests |
| 3. MoltenVK in-process, a triangle into an `MTLTexture` | **Not started**. No core involved; this is where the zero-copy handoff is proven or corrected |
| 4. `SET_HW_RENDER` for Vulkan, against Beetle PSX HW | **Not started**. Deliberately a core that is not N64, so a wrong contract shows up on a game whose software path already works |
| 5. Recompiler measurement | **Not started** |
| 6. **paraLLEl-N64**, the N64 | **Not started**. See the two gates below |
| 7. ANGLE alongside MoltenVK | **Partly moot**. It existed for the DS, which was reached without it. Still needed by a GL-only core later |
| 8. **Citra**, the 3DS | **Not started**. Two screens of unequal width, which the DS does not expose |
| 9. **PPSSPP**, the PSP | **Not started** |
| 10. **Switch**, stage 1: the wrapper against a stub engine | **Partial**. `native/switch-wrapper/` already has the `retro_*` skeleton, the frame gate, a Vulkan stub renderer and a test harness. No Rust engine behind it |
| 11. Switch, stage 2: a real engine behind `ISwitchEngine`, homebrew booting | **Not started** |
| 12. Switch, stage 3: capability shim, shader cache, retail content | **Not started** |

### The Switch, since it is the one that gets misread

**It is the last and hardest SYSTEM TO EMULATE, after N64. It is not a platform Continuum runs on.**
Section 12 of the design document is the whole approach: every existing Switch engine is a
standalone application rather than a plugin, so `continuum_switch_libretro.cpp` wraps one behind an
`ISwitchEngine` interface and hands it to the rest of Continuum as an ordinary libretro core, which
is why nothing else in the engine has to learn that anything unusual happened.

Section 11 of that document also establishes it fits in memory, with the entitlements, on 12 GB
hardware and only there: roughly 4 GB of guest RAM plus 1 to 2 GB of GPU resources plus recompiled
code, against a 6 to 8 GB working budget.

### Two things gate N64, and neither is graphics

**The recompiler, and it may not be winnable on a sideloaded build.** An N64 interpreter is far
too slow, so N64 needs a working JIT.

Worth knowing before any more graphics work is done for it: the entitlements file asks for
`com.apple.security.cs.allow-jit` and `com.apple.security.cs.allow-unsigned-executable-memory`,
and **both are macOS hardened-runtime keys that iOS ignores.** iOS gates executable memory behind
`dynamic-codesigning`, which no provisioning profile can carry, at any account tier. Only a signing
bypass such as TrollStore, or a jailbreak, grants it. So on a normal sideload, signed on-device with
a developer or distribution certificate, the honest expectation is that a recompiler cannot run at
all.

That would put N64 out of reach on such an install, and the same applies to PSP, 3DS and the Switch,
which all want a recompiler too. It does NOT affect anything shipping today: every one of the
fourteen systems runs on an interpreter. The button in Settings settles it per install, and that
answer should be had before steps 3 to 6 are built for a core that could not execute. The
entitlements have claimed one since the first build and had never been exercised, so this build
carries a probe: open a game, tap ⓘ, read the line starting `JIT:`. That is item 1 in
[TESTING.md](TESTING.md)'s queue and it is the cheapest useful thing anyone can do right now.

Reading the cores established the rest: `pcsx_rearmed` and `mupen64plus-next` have **no** Apple JIT
support at all, so switching the PlayStation recompiler on would have failed at the first
executable page and looked like a broken core. `parallel-n64` **does** have it, and it is also the
core the graphics design already chose for unrelated reasons. Its iOS build disables the recompiler,
so enabling it means porting that core's macOS arm64 configuration, which is bounded work on the
same CPU under the same rules.

**MoltenVK in the bundle**, roughly 8 MB on an app that is currently 7.3 MB. Unavoidable:
paraLLEl-RDP is Vulkan compute and has no GL equivalent.

---

## Known problems

- **The app would not open, and it was the JIT probe.** Fixed. That probe writes a function into
  memory and calls it, and it ran at STARTUP. Executing a page the process just wrote is the one
  thing iOS terminates an app for unless the dynamic-codesigning entitlement is genuinely in force,
  and whether it is depends on how the copy was signed and installed rather than on the build:
  TrollStore preserves it, a free developer account does not. So on those installs the app opened
  and was killed before drawing anything, including the line the probe existed to print. **That
  silently blocked every on-device test for several builds**, which is also why the JIT line never
  came back. Startup now only asks whether such a page can be MAPPED, which is a real answer and
  cannot get the process killed; the half that runs code is a clearly labelled button in Settings.

- ~~**mgba is flaky in CI.**~~ **Diagnosed and fixed.** The archive holds LLVM bitcode, because
  mgba's CMake adds `-flto` unconditionally on Apple through a condition that parses as
  `APPLE OR (GNU AND BUILD_LTO)`, so `-DBUILD_LTO=OFF` could never have turned it off.
  `-force_load` guaranteed the archive members were loaded and then LTO decided what to emit from
  them, and since nothing in the link referenced the libretro API, LTO was free to internalise it.
  Whether it did came down to how its parallel codegen partitioned the module, which is why the
  same commit passed and failed on different days. Every entry point is now named with `-u`, which
  makes it a root before LTO runs. One clean build so far, and the mechanism is deterministic
  rather than lucky.
- **The staged-dylib check now covers all 20 entry points**, not just `retro_run`. One symbol only
  ever caught a core that resolved nothing; a core that loses one entry point is both likelier and
  much quieter. That is not hypothetical: save states were broken for this project's whole life
  because `retro_serialize` was never resolved.
- **`swiftc` on Linux cannot type-check.** It is a syntax pass, so a Swift type error, an actor
  isolation mistake or a wrong argument label is only caught by CI. Several builds have been spent
  on exactly that, and it is a property of the toolchain rather than carelessness.
- **The six core sources are not pinned.** Each is cloned at whatever its default branch's HEAD is
  on the day CI runs. Pinning six upstreams means maintaining six pins and missing their fixes, so
  the tradeoff is deliberate, but it does mean a core that built last week can change under us. It
  matters most for the DS, where two option VALUES are hardcoded here and compared by string inside
  melonDS: an upstream rename would switch the touch screen off again with no error at either end.
  Every build now records each core's repository and commit in `core-sources.txt` inside the build
  metadata artefact, so that becomes a diff between two builds rather than a mystery.

---

## Deliberately deferred

Not forgotten, and not bugs:

- iOS deployment target stays at 16 rather than 18, and Swift stays at language mode 5.
- Per-orientation control layouts.
- Nothing to do with PSP, 3DS or Switch is deferred; they are steps 9 to 12 of the road above.
