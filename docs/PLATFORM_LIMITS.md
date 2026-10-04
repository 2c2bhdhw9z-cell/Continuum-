# What this device can and cannot do, with the evidence

Written after a long session of getting this wrong twice. Everything here is either measured on the
owner's iPhone 17 Pro Max or read from a primary source, and each claim says which. **Do not
re-derive these from first principles; check them against this page first.**

Nothing here is a guess. Where something is unknown it says so.

---

## The device and the install

| Thing | Value |
| --- | --- |
| Device | iPhone 17 Pro Max, `Apple A19 Pro GPU / Metal` (reported by the app) |
| Signing | Paid certificate from a UDID registration service, signed ON DEVICE with ESign |
| Certificate used | **Distribution.** The Development one will not install |
| Developer Mode | Already ON. Ruled out as the cause of the development-cert failure |
| No computer | Everything is done from the phone. CI is the only compiler. The owner will not use or borrow a computer, not even once (see "JIT is not reachable for this owner") |

The development certificate fails with a verification/integrity error. Ruled out so far: Developer
Mode being off, and ESign's "Auto modify jailbreak dependencies" toggle (tested both on and off).
**Still untested:** whether that service's development profile actually contains this device's UDID,
and whether the `.p12` and `.mobileprovision` are from the same certificate.

---

## JIT: the mechanism, and why it matters

Two earlier readings of this were wrong in opposite directions. This is the verified one.

**iOS does NOT gate executable memory on the `com.apple.security.cs.*` keys** in
`native/ios/Continuum.entitlements`. Those are macOS hardened-runtime keys and iOS ignores them.

**It does NOT require `dynamic-codesigning` either.** That is the TrollStore/jailbreak route and it
is not the only one.

**What it requires is `get-task-allow` plus an attached debugger.** A debugged process gets
`CS_DEBUGGED` and the kernel permits executable memory. StikDebug and StikJIT do the attaching
on-device over a local VPN loopback; a computer is needed only once, to produce a pairing file.

`get-task-allow` is grantable **only by a development provisioning profile**. Apple does not
whitelist it on distribution profiles.

> Sources: StikJIT's integration guide states the process receiving JIT must have `get-task-allow`.
> Apple's developer forums state that entitlement is whitelisted only by development profiles.

### Measured on the device

The app reads its own signature through `csops(CS_OPS_STATUS)` and prints the result. On the current
distribution-signed install it reports:

```
JIT: get-task-allow is MISSING, so nothing can attach a debugger and no recompiler can run.
Re-sign with a DEVELOPMENT certificate and profile, not a distribution one.
Mapping: MAP_JIT refused, plain executable mapping accepted.
```

Note the second half: a plain executable mapping being **accepted** is about allocating memory, not
permission to run it. Do not read it as JIT being available.

### Why AltStore and SideStore users get JIT

They sign with a **free Apple ID**, which issues a **development** certificate, which carries
`get-task-allow`. That is the whole trick, and it is why DolphiniOS works for people on
non-jailbroken phones. It is a signing difference, not a device difference.

### One further requirement on this hardware

Where TXM/SPTM is present, which it is on recent Apple silicon, attaching a debugger is **not
sufficient**. Each executable region must be prepared over the debug connection first, through a
breakpoint protocol the HOST APP implements:

```c
JIT26Detach()                      // mov x16, #0 ; brk #0xf00d ; ret
JIT26PrepareRegion(address, length) // mov x16, #1 ; brk #0xf00d ; ret
```

**Not implemented in Continuum.** It is Part 1 of StikJIT's integration guide. A `brk` with no
script attached crashes the process, so this is gated work rather than a flag.

### JIT is not reachable for this owner

The owner's stated position (29 Sept 2026): no computer, not even once, and no library or
borrowed machine. That alone rules JIT out, whatever certificate is bought:

- **The pairing file needs a computer next to the phone.** On iOS 26.x it is made over a USB
  cable. From iOS 27, idevice_pair can pair over Wi-Fi with no cable, but only from a computer on
  the same local network. A cloud machine (CI, or an AI agent's VM such as Grok Bot's) can do
  neither. A pairing file also grants deep access to the device, so never make or store one on a
  shared online machine.
- **"Once" is not guaranteed.** Pairing files can expire at random and then have to be remade.
- **A signing service's "JIT certificate" only supplies `get-task-allow`.** It does not remove
  the pairing file. Do not suggest buying one to this owner.
- **The app side is missing too** (the TXM protocol above), and it could not be tested anyway.

So every system must be usable with no JIT: interpreters and cached interpreters only. The system
this really hurts is the **Switch**. iPhone Switch emulators such as MeloNX require JIT, so raise
this with the owner before starting steps 10 to 12 of
[SET_HW_RENDER_DESIGN.md](SET_HW_RENDER_DESIGN.md) section 13. If the owner's position changes,
JIT becomes a real project again, starting with the TXM protocol.

> Sources: the idevice_pair README (Wi-Fi pairing needs iOS 27 and the same network), the
> StikDebug guide's pairing-file page (files expire at random), and StikJIT's INTEGRATION.md.

---

## Which cores actually build for iOS

**The libretro buildbot is the authority, not a makefile reading.** It publishes builds for
`ios-arm64`, and at the time of writing there are **178** of them:

```
https://buildbot.libretro.com/nightly/apple/ios-arm64/latest/
```

| Core | System | On the iOS buildbot |
| --- | --- | --- |
| `parallel_n64` | N64 | **yes** |
| `mupen64plus_next` | N64 | yes |
| `yabasanshiro`, `yabause` | Saturn | yes |
| `mednafen_psx`, `mednafen_psx_hw` | PS1 | yes |
| `fbneo`, `fbalpha2012_cps1/2/3`, `fbalpha2012_neogeo` | Arcade, Neo Geo | yes |
| `mame2003_plus`, `mame2010`, `mame2016`, `hbmame` | Arcade | yes |
| `desmume` | DS | yes |
| `citra` | **3DS** | **NO** |
| `flycast` | Dreamcast | **NO** |
| `ppsspp` | PSP | **NO** |
| `dolphin` | GameCube, Wii | **NO** |

Check this list before promising or refusing a system. It is one `curl` away.

**Re-checked 4 October 2026:** still 178 cores, and azahar/citra, flycast, ppsspp and dolphin are
still absent. Continuum does not depend on the buildbot for those: CI compiles **azahar**,
**flycast** and **ppsspp** from source (`scripts/build-core.sh`), and all three are in the IPA
(32 cores in build 120). Dolphin is not built (see docs/HARD_SYSTEMS.md).

**Continuum ships `mednafen_psx_hw` (Beetle PSX HW) through `IOS_CORES` / `ios-all`.** The
dylib is built only on a Mac host or the GitHub Actions `macos-latest` runner
(`make platform=ios-arm64 HAVE_HW=1`); a Linux box cannot cross-compile it. Soft
`pcsx_rearmed` remains the default PlayStation route; Settings can select Beetle for
step 4 `SET_HW_RENDER` exercises. That path is still Partial until a device frame
proves it.

---

## N64 is possible right now, with no JIT and no graphics work

`parallel-n64`'s own iOS block in its `Makefile` sets:

```make
HAVE_OPENGL=0     # software renderer, so it uses the same pixel path the other cores do
WITH_DYNAREC=     # interpreter, so it needs no executable memory at all
HAVE_PARALLEL=0
```

So it needs **neither** the MoltenVK work nor `get-task-allow`. It will be slow, because a software
rasterizer plus an interpreter on any phone is slow, but it runs.

Facts read from the core: `valid_extensions` is `n64|v64|z64|bin|u1|ndd|zip`, `need_fullpath` is
true, the pixel format is XRGB8888, and geometry, aspect and frame rate are all reported
dynamically rather than fixed. It has submodules.

`.bin` and `.zip` must NOT be routed: `.bin` is a PlayStation cue sheet's companion track here and
must never be tappable.

---

## 3DS: upstream libretro citra does not build; Azahar does

**The libretro citra core does not build for iOS.** Its makefile has platform blocks for unix, osx,
libnx and MSVC and none for iOS, it uses GLAD to load OpenGL, and it is absent from the buildbot.
That is still true, and it is not the core this app ships.

**Azahar** (the maintained Citra fork) has an iOS arm64 libretro job. Its Apple build turns OpenGL
off and Vulkan on, presents with `retro_vulkan_image` `set_image`, and compiles the CPU JIT out
under `IOS`. That is the core in the IPA (`azahar_libretro_ios.dylib`). It is not device-proven.
An OpenGL 3DS core would still boot a black screen here: this host refuses every `SET_HW_RENDER`
context that is not Vulkan.

**Standalone 3DS emulators for iPhone exist and ship**, which is direct proof the system itself is
viable on this hardware:

- **Folium** — on the App Store, Citra-based, lists Nintendo 3DS and New Nintendo 3DS among its
  supported systems alongside DS, GBA, NES, SNES, Genesis and PS1.
- **emuThreeDS** — a Citra fork for Apple devices, and **open source**, so the iOS-specific work is
  readable rather than guessed at.
- **Manic EMU** — another current App Store option with 3DS support.

Folium, emuThreeDS and Manic remain proof the system runs on an iPhone at all. What this app
actually shipped is narrower:

1. **Vulkan `set_image`**, which steps 3 and 4 already accept. Azahar asks for that context.
   OpenGL is compiled out of the iOS core, so this row cannot fall through to a context the
   host refuses.
2. **Azahar's own iOS libretro target**, not a new port of upstream citra.
3. **No JIT.** Dynarmic stays off. The fast interpreter is the speed this signed IPA can have.
   Do not turn JIT on to make it faster.

Not device-proven. See STATUS.

---

## Systems ranked by what they actually need

| Needs nothing new | Needs the graphics path | Needs graphics AND a core port |
| --- | --- | --- |
| **N64** (software + interpreter). **3DS** is in the IPA via Azahar Vulkan, untested, no JIT | Dreamcast, PSP | GameCube, Wii (3DS no longer sits here) |
| Saturn, Neo Geo, CPS1/2/3, MAME | | |
| Sega CD, 32X, and the 8/16-bit long tail | | |

---

## Things that turned out to be true and are worth not relearning

- The app opening and closing instantly was the JIT probe **executing generated code at startup**.
  iOS kills a process for that. Startup now only asks whether a page can be mapped.
- An Actions **artifact** cannot be installed from a URL: it needs GitHub authentication and it is a
  zip around the `.ipa`. A **Release asset** on a public repo is plain HTTPS, and
  `/releases/latest/download/Continuum.ipa` always redirects to the newest build.
- ESign's custom entitlements box wants a **file**, not pasted text. `native/ios/get-task-allow.plist`
  exists for exactly that and is attached to every Release.
- A core declares how it wants content. `need_fullpath` true means hand over a path; false means the
  frontend owes it the bytes. Stella was the first core here that needed the bytes.
- Check a core's own `valid_extensions` before concluding a system needs a new core. The Famicom Disk
  System and the SG-1000 were already supported by cores in the app and only needed routing.
