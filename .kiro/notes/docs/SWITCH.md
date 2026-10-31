# The Switch: nxnative, the ways in, and the owner's open questions

Started 31 October 2026, 1:45 PM Eastern. The owner: "Start the switch stuff. Use this as
Reference maybe. https://github.com/seedds/nxnative". Nothing is built yet. The owner's rule says
to raise JIT with him before starting the Switch, so the four questions at the bottom went to him
first. His answers decide the way in. `native/switch-wrapper/` (road step 10, a stub engine) is
unchanged.

## What nxnative is

nxnative does not emulate the Switch. A Switch game is ARM64 code and so is an Apple chip, so it
runs the game's own code directly:

- **The rewriter** (`dm/tools/patch_nso.py` and helpers, Python with Capstone) patches only the
  instructions Apple's kernel can't run: system calls (`svc`), reads of the thread-local
  register, and uses of `x18`, which iOS keeps for itself. It writes stubs into a patch area
  next to each module.
- **The host** (`dm/host`, about 23,500 lines of C and Objective-C) stands in for Horizon, the
  Switch's system: threads, memory, files and saves, controllers, sound, movies (VP9 and Vorbis),
  the keyboard, cheats.
- **The graphics** (`dm/gfx`, about 54,500 lines) are NVN, the Switch's graphics library,
  rebuilt on Metal. Shaders are translated ahead of time with Ryujinx's shader translator
  (`dm/third_party/ryujinx`, about 64,700 lines of C#, MIT), then SPIRV-Cross and glslang, then
  Xcode 27.1's Metal compiler.
- **One profile per game** (`dm/games/<title id>/`, 125 folders; its README lists 112 tested
  games): build IDs, per-build patches, memory sizes, shader tables. Behaviour that only fits one
  game is gated by that game's build ID. The host is compiled per game (`make -C dm/host
  GAME=<id>`, constants from `game_profile.h`). A game with no profile gets a guessed one, and
  "such an IPA may not start".
- **On iOS** each game becomes its own app. `dm/ios/tools/gen_embed.py` puts the rewritten code
  in the app's own `__TEXT`, so the sideloader signs it with the app. At start `dm/host/loader.c`
  reserves one range and `vm_remap`s the pieces back to their original places: code pages
  shared with the signed pages (copy = FALSE, r-x), data copy-on-write. So the game sees its
  original layout and its PC-relative addressing needs no change. This works on iOS 26 with no
  JIT. A 5 GB guest block is the most proven on a phone (`docs/reference/memory.md`).
- **Needs** an iPhone 15 Pro or newer (A17 Pro), or an M1 iPad, on iOS 26 or later, because the
  games' textures are BC-compressed. The owner's iPhone 17 Pro Max on iOS 27.0.1 qualifies.
  32-bit games can never run this way (Pikmin 3 Deluxe, New Super Mario Bros. U Deluxe, Mario
  Kart 8 Deluxe 1.0.0). No firmware: only the player's `prod.keys` and the game (NSP, XCI, NSZ,
  XCZ, plus update and DLC).
- **Every way to use it needs a computer**: the script on a Mac (macOS 26.6, Xcode 27.1,
  Homebrew, Python 3.12, the .NET SDK), or NXNativeWindows on a PC. The front ends (NX2Apple,
  NX Play, NxNative Launcher, LumaNX, LumaDock) are all Mac apps. Nobody converts a game on the
  phone. A first build takes 30 to 60 minutes on a Mac and about twice the game's size plus 10 GB
  of free space.
- **Licences**: nxnative is MIT. What it carries is MIT, BSD-3, zlib, Apache-2.0 or public
  domain (`dm/third_party/README.md`). None of it would stop Continuum going closed source later,
  as long as the notices stay in the credits.

The clone used for reading is `/projects/sandbox/.scratch/nxnative` (commit 5474c6f, 31 October
2026). Never commit anything from it here. Its own agent guide is `docs/agents/README.md`; per-game
knowledge is in `docs/agents/game-patches.md` and the `docs/reference/` pages.

## Why it can't go into Continuum as it is

Continuum is one app, and games are added after it is installed. An iPhone only runs code that
was signed, unless JIT is on. nxnative gets round that by signing each game's code inside that
game's own app, on a computer. Continuum has neither the computer nor the per-game app, so two
things have to change.

### 1. Running the converted code: a certificate, or JIT

- **The player's own certificate.** Continuum writes the converted code into a library, signs
  it on the phone with the certificate the player signs Continuum with (the .p12 and its
  password, imported once; the same team, so iOS lets the app load it), loads it, and remaps its
  code pages into the game's layout exactly as nxnative's `loader.c` does. Both halves have been
  shown on iOS 26 and later by other apps. LiveContainer signs the apps it runs on the phone with
  the player's certificate (its README: below iOS 26, JIT skips signing; otherwise apps are
  signed with LiveContainer's certificate). nxnative shows the remap. Nobody has put the two
  together, so it is the first thing to prove on the owner's phone. It needs no JIT, so players
  without JIT could play too. Signing code: apple-codesign (rcodesign's Rust crate), zsign, or
  the engine of the owner's own Signer app (repo snes-tas-studio).
- **JIT.** `crates/emulator-bridge/src/jit26.c` blesses one 512 MB region before the debugger
  lets go, and hands out slices with one address to run from and one to write through. A Switch
  game needs its data at fixed distances after its code, and a big game's main module can be
  over 300 MB (Monster Hunter Rise's is 314 MB). So the game's memory would have to be laid out
  round a blessed region: either bless the exact code ranges when the game starts, or bless a
  much bigger region. Whether remapped blessed pages stay runnable under TXM is not known. It
  has to be measured on the phone.

### 2. Converting on the phone

- The rewriter, the unpacking and the profile logic are Python. They would be rewritten in Rust,
  or run in an embedded Python (CPython runs on iOS officially since 3.13).
- The shader translator is C#. It could be built for iOS with .NET NativeAOT (MeloNX ships .NET
  on iOS), or rewritten.
- Metal libraries would be compiled on the phone (`newLibraryWithSource`, kept in binary
  archives) instead of with Xcode's compiler.
- The host is compiled per game today. One Continuum core would read the profile when the game
  starts.
- nxnative draws with Metal into its own layer. Continuum's engine takes libretro frames
  (pixels, or a Vulkan image turned into an `MTLTexture` with `VK_EXT_metal_objects`). A Metal
  core needs a way to hand over its texture. That is a design decision for later.
- Converting will take a while, once per game, and needs the free space.

## The other way: an emulator

MeloNX (Ryujinx's code, C# and Swift) is the iPhone Switch emulator people use. It needs JIT
and can't run without it, and it recommends an iPhone 15 Pro or newer. It fits the old plan,
`SET_HW_RENDER_DESIGN.md` section 12 (the C# NativeAOT row) and `native/switch-wrapper/`: one
emulator wrapped as a libretro core. It plays more games in principle, but it translates every
instruction instead of running the game's own code, so it is slower. Ryujinx is MIT. MeloNX's own licence is unclear (GitHub shows
NOASSERTION for AzureDominus/melonx and MIT for the nurtrino fork), so check it before using it.

## The questions put to the owner (31 October 2026), still open

1. Which way?
   A. nxnative's way: the game's own code on the iPhone's chip. Fastest, but only games someone
      has fixed up (112 on its list now); other games probably won't start, and 32-bit games
      never will.
   B. An emulator, MeloNX's way: more games, slower, JIT only.
   C. A first, then B for the games A can't play.
2. Whose code?
   A. Our own, written from scratch, with nxnative only read to learn how it's done (like cBoy).
      The slowest way to a first game: the Switch is far bigger than any core so far.
   B. Built on nxnative's code and credited, the way Continuum Symbian is built on EKA2L1. The
      fastest. Its MIT licence lets it stay when Continuum goes closed source.
   C. B first, then our own pieces replace it over time.
3. How the converted game gets to run?
   A. JIT only. Players without JIT get one short line.
   B. The player's own certificate only (works without JIT; each player imports theirs once).
   C. Both: the certificate when one is imported, JIT otherwise.
4. Which Switch games does the owner have dumped, with his prod.keys? The first test should be a
   game on nxnative's tested list. Games and keys never go into the repo or the cloud, so his
   phone is the only real test.

The recommendation given with them: 1C, 2C, 3C, and the first game is whichever of his is on
nxnative's list.

## When the answers come

Record them here and in STATUS.md "Next up". Whatever he picks, the first proof is on his phone:
for the certificate way, load one signed library made on the phone and run a few instructions
from its remapped code; for JIT, the same from a blessed region laid out the Switch way. Then
reading his NSP with his prod.keys on the phone (title, icon, version, the code modules), then
the first converted game.
