# What is finished, and what is not

One page, kept current, so nothing has to be inferred from a commit log. Three states only:

- **Done** means built, in the `.ipa`, and confirmed working on a device.
- **Built, untested** means it is in the app and should work, and nobody has tried it yet. Those
  are the rows in [TESTING.md](TESTING.md)'s queue.
- **Partial** means some of it works and a named piece is missing. Read the note.

If a row says Partial, the note says exactly what is absent. Nothing here is rounded up.

---

## Systems

Six cores, ten systems.

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
| Core setting overrides | **Done** | The host refuses a core's requests for its settings, so every core keeps its own defaults. Two DS settings had to be answered because they do not start at the default they advertise; everything else, for every core, is still refused |
| Multi-screen compositor | **Done, unused** | Draws N regions of one texture to N places. Nothing selects more than one yet: it exists for rearranging the DS screens and for hardware-rendered cores |
| Nintendo Switch | **Partial** | The NEXT platform after the iPhone app. `native/switch-wrapper/` is a working frame gate and a Vulkan stub with no engine behind it yet. See Platform order below |
| Android `.apk` | **Not started** | Far future, after the Switch. Everything new goes in the Rust engine, so Android inherits it whenever it is picked up |

---

## The road to N64

N64 is **step 6** of the twelve-step sequence in
[docs/SET_HW_RENDER_DESIGN.md](docs/SET_HW_RENDER_DESIGN.md) section 13. The DS did not need any of
it, which is why the DS arrived first.

| Step | State |
| --- | --- |
| 1. wgpu owns the one `MTLDevice` | **Done** |
| 2. Composite pass generalised to N screens | **Done**, with 7 layout tests and 3 shader tests |
| 3. MoltenVK in-process, a triangle into an `MTLTexture` | **Not started**. No core involved; this is where the zero-copy handoff is proven or corrected |
| 4. `SET_HW_RENDER` for Vulkan, against Beetle PSX HW | **Not started**. Deliberately a core that is not N64, so a wrong contract shows up on a game whose software path already works |
| 5. Recompiler measurement | **Not started** |
| 6. **paraLLEl-N64** | **Not started** |

### Two things gate it, and neither is graphics

**The recompiler.** An N64 interpreter is far too slow, so N64 needs a working JIT. The
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

## Platform order

Which devices Continuum itself runs on, in the order they are being done. Not to be confused with
the systems it emulates, which is the table at the top.

| Order | Platform | State |
| --- | --- | --- |
| 1 | **iPhone (`.ipa`)** | The one being built. Everything above refers to this. |
| 2 | **Nintendo Switch** | Next, and a genuine attempt rather than a promise. `native/switch-wrapper/` already holds a frame gate, a Vulkan stub renderer and a test harness; what it does not have is the Rust engine wired in behind it. |
| 3 | Android (`.apk`) | A long way off. Deliberately last. |

The reason this order costs little: every feature is built in the Rust engine rather than in Swift
wherever there is a choice. Audio, input, timing, rewind, save states, cheats and the compositor are
all engine-side, so a new platform needs a shell and a screen, not a rewrite. The iPhone app is the
shell that exists today.

---

## Deliberately deferred

Not forgotten, and not bugs:

- iOS deployment target stays at 16 rather than 18, and Swift stays at language mode 5.
- Per-orientation control layouts.
- PSP and 3DS as emulated systems. The Switch is NOT in this list; it is the next platform to run
  Continuum ON. See Platform order.
