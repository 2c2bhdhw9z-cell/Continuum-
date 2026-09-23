// Continuum, the Phase 5 Step 2 host app.
//
// Step 10 proved the chain with a stub: the C++ libretro wrapper rendered a rotating colour,
// the Rust engine drove it and composited, and this presented the result on a CAMetalLayer.
// Step 2 swaps that stub for the first real libretro core, PCSX ReARMed (PS1), through the
// same software frame path. If the HUD is counting frames, the loader, the pixel-format
// negotiation and the audio pipeline are all working against a real core.
//
// The stub set SET_SUPPORT_NO_GAME and happily booted with empty content, so the host used
// to declare, load and launch straight from the attach callback. PCSX ReARMed does not: it
// declares need_fullpath and hard-rejects a NULL/empty info->path with
// "retro_load_game rejected the content". So the host does not auto-boot. The surface attach
// only makes the core resident (declare + load); the actual launch waits for the user to
// choose a game, and then hands the core that game's real absolute path.
//
// IMPORT AND COPY, NOT STREAM IN PLACE. This is the architecture that replaced a chain of
// on-device failures, and the reasons matter because they are exactly the dead ends not to
// walk back into:
//
//   * Earlier builds picked a FOLDER through UIDocumentPickerViewController and held a
//     folder-level security scope for the whole session, so the C++ core could fopen a .cue
//     plus its sibling .bin tracks where they lay. On device that failed over and over: the
//     folder pick came back cancelled, or the pick callback never arrived at all.
//   * Security-scoped streaming from outside the app sandbox is simply too fragile on modern
//     iOS to ship. Every read depends on a grant that can be refused, and when it is refused
//     the C++ side just sees a failed fopen.
//   * Asking a user to hand-move folders around in the Files app is not a release-grade
//     experience in the first place.
//
// So the host now behaves the way Delta and Provenance do. The user picks FILES, multi-select,
// so a .cue and every .bin track it names come in together. The app copies them permanently
// into its own Documents directory. Games are launched from there, and inside our own sandbox
// there is no security scope to be denied: the core opens the .cue and each adjacent .bin with
// a plain fopen. A Library view lists what has been imported, which is also the only place the
// launch decision is made now.
//
// FIVE CORES, ONE APP, ONE LOADED AT A TIME. The .ipa now carries every core the web build
// has, plus PS1: fceumm (NES), snes9x (SNES), mGBA (GBA and GB/GBC), Genesis Plus GX (Mega
// Drive, Master System, Game Gear) and PCSX ReARMed (PS1). All five are declared to the
// engine when the surface attaches, because declaring is metadata only and loads no code, and
// then exactly one of them is loaded: the one the tapped game needs, chosen from its file
// extension through the single routing table in `CoreCatalog.routes`. That is the whole
// multi-system story, and the two rules that keep it working are that the routing table has
// exactly one copy and that `engine.coreState` is the only thing ever asked whether a core is
// resident.

import QuartzCore
import SwiftUI
import UIKit
import UniformTypeIdentifiers

@main
struct ContinuumApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
                // Always dark: the app surrounds emulated output, and a light letterbox around a
                // dark game is glare rather than design.
                .preferredColorScheme(.dark)
                .statusBarHidden(true)
                .ignoresSafeArea()
        }
    }
}

// MARK: - What the picker handed back

/// A `Sendable` snapshot of the file picker's result.
///
/// `UIDocumentPickerDelegate`'s methods carry no actor isolation that the compiler can see, so
/// what they receive has to cross a hop to reach the `@MainActor` engine host. Only Sendable
/// data travels: any failure is flattened to its finished HUD line right where it is received,
/// because `any Error` is not `Sendable`. `URL` is `Sendable`, so the picked URLs pass through
/// untouched, which is what lets the delegate hand the whole multi-file selection to the main
/// actor unchanged and in order.
struct PickerOutcome: Sendable {
    /// Exactly what the picker returned, in order. Empty when the picker failed.
    let urls: [URL]
    /// Nil on success. On failure this is the HUD line, already built.
    let failureText: String?
}

// MARK: - The cores this build ships

/// One libretro core in the bundle, as declared to the engine before anything is loaded.
///
/// THE NUMBERS BELOW ARE PRE-LOAD HINTS, AND THAT IS NOT A DEFECT TO CORRECT LATER.
/// `declareCore` sizes an initial GPU texture from the geometry so the first frame has
/// somewhere to land, and then `sync_descriptor_from_core` overwrites geometry, fps and sample
/// rate from `retro_get_system_av_info` the instant the core loads. The pixel format is
/// renegotiated through `SET_PIXEL_FORMAT` inside `retro_load_game`, and native_core.rs reports
/// whatever the core actually chose. So the two fields that have to be exactly right are
/// `coreId` and `library`: everything else is superseded by the core itself, one call later.
///
/// `pixelFormat` is the BRIDGE's numbering, not libretro's: 0 = RGB565, 1 = XRGB8888,
/// 2 = RGBA8888, matching `PixelFormat::as_u32` in the Rust engine and the `pixel_format`
/// field on `CoreDeclaration`. Do not "fix" these into libretro's RETRO_PIXEL_FORMAT_* values,
/// which number the same three formats differently.
struct CoreSpec: Sendable {
    /// The registry id, and the string every HUD line names so a routing mistake is visible.
    let coreId: String
    let displayName: String
    /// The systems this core claims. Matches web/cores/manifest.json for the four cores the
    /// web build ships too, so the two builds cannot disagree about what runs what.
    let systems: [String]
    /// The dylib in Frameworks/, dlopened at runtime.
    ///
    /// MUST match the canonical filename scripts/build-core.sh stages, byte for byte.
    /// `scripts/build-core.sh ios-names` prints that list, project.yml embeds it,
    /// package-ipa.sh falls back to it and .github/workflows/ios.yml asserts it. A single
    /// wrong character here reads on the HUD as a core missing from a bundle it is in.
    let library: String
    let width: UInt32
    let height: UInt32
    let maxWidth: UInt32
    let maxHeight: UInt32
    let aspectRatio: Float
    let fps: Double
    let sampleRate: UInt32
    let pixelFormat: UInt32
    /// Higher wins when two cores claim the same system. Nothing overlaps here, so this only
    /// matters if a second core for one of these systems is ever added.
    let priority: Int32
    /// BIOS filenames this core looks for in the system directory, most useful first. Empty
    /// for a core that needs none, which is every core here except PCSX ReARMed. Never
    /// bundled: shipping a console BIOS is a copyright violation.
    let biosNames: [String]
}

/// The five cores, and THE extension-to-core routing table.
///
/// Top level rather than nested privately inside `EngineHost` because the Library rows read the
/// same routing table the launch path does. A second copy of that mapping is precisely the bug
/// this type exists to prevent: it would stay invisible until a .sms opened on the wrong core.
enum CoreCatalog {
    /// NES. Note for anyone reading a device log: the iOS makefile for this core forces
    /// WANT_32BPP, so on device it negotiates XRGB8888 even though it is declared RGB565 here
    /// and built RGB565 for the web. The negotiation inside `retro_load_game` settles it and
    /// the renderer converts either, so this is correct rather than a mismatch to fix.
    static let fceumm = CoreSpec(
        coreId: "fceumm",
        displayName: "FCEUmm (NES)",
        // "fds" as well as "nes", because this core declares `fds` in its own valid_extensions and
        // the Disk System is routed to it. The engine uses this list to answer "which cores can run
        // this system", so omitting it would leave a system the app routes here unclaimed by any
        // core as far as the engine is concerned.
        systems: ["nes", "fds"],
        library: "fceumm_libretro_ios.dylib",
        width: 256, height: 240,
        maxWidth: 256, maxHeight: 240,
        aspectRatio: 1.2195,
        fps: 60.0988,
        sampleRate: 48000,
        pixelFormat: 0,
        priority: 100,
        biosNames: []
    )

    /// SNES. The max geometry is the hi-res interlaced worst case (1024x478), which is what
    /// the web manifest declares for the same core.
    static let snes9x = CoreSpec(
        coreId: "snes9x",
        displayName: "Snes9x (SNES)",
        systems: ["snes"],
        library: "snes9x_libretro_ios.dylib",
        width: 256, height: 224,
        maxWidth: 1024, maxHeight: 478,
        aspectRatio: 1.3333333,
        fps: 60.0988,
        sampleRate: 32040,
        pixelFormat: 0,
        priority: 100,
        biosNames: []
    )

    /// GBA plus GB/GBC. 65536 Hz is not a typo: mGBA reports 2^16, and the resampler in the
    /// engine is what turns that into the device's rate.
    static let mgba = CoreSpec(
        coreId: "mgba",
        displayName: "mGBA (GBA, GB, GBC)",
        systems: ["gba", "gb", "gbc"],
        library: "mgba_libretro_ios.dylib",
        width: 240, height: 160,
        maxWidth: 240, maxHeight: 160,
        aspectRatio: 1.5,
        fps: 59.7275,
        sampleRate: 65536,
        pixelFormat: 0,
        priority: 100,
        biosNames: []
    )

    /// Mega Drive, Master System and Game Gear from one core. The base geometry is the Mega
    /// Drive's 320x224; the declared max is 348x240 so a 256x192 Master System frame and a
    /// 320x240 Mega Drive frame both fit inside the texture the declaration sizes, whichever
    /// arrives first. The core picks which system to be from the content extension, which is
    /// why the launch path hands the real filename through to `ContentHint::from_filename`.
    static let genesisPlusGx = CoreSpec(
        coreId: "genesis_plus_gx",
        displayName: "Genesis Plus GX (Mega Drive, Master System, Game Gear)",
        systems: ["genesis", "sms", "gg", "sg1000"],
        library: "genesis_plus_gx_libretro_ios.dylib",
        width: 320, height: 224,
        maxWidth: 348, maxHeight: 240,
        aspectRatio: 1.3333333,
        fps: 59.9227,
        sampleRate: 44100,
        pixelFormat: 0,
        priority: 100,
        biosNames: []
    )

    /// PS1, and the first real core this app ever loaded. Values unchanged from the build that
    /// worked: native 320x240, a framebuffer that widens to 640x480 and, with interlace and
    /// overscan, to about 700x576. A real BIOS in the system dir raises compatibility, and
    /// without one PCSX ReARMed falls back to HLE and still boots, which is why a missing BIOS
    /// is a HUD note rather than an error.
    static let pcsxReARMed = CoreSpec(
        coreId: "pcsx_rearmed",
        displayName: "PCSX ReARMed (PS1)",
        systems: ["ps1"],
        library: "pcsx_rearmed_libretro_ios.dylib",
        width: 320, height: 240,
        maxWidth: 700, maxHeight: 576,
        aspectRatio: 4.0 / 3.0,
        fps: 59.94,
        sampleRate: 44100,
        pixelFormat: 0,
        priority: 0,
        biosNames: ["scph1001.bin", "scph5501.bin", "scph7001.bin",
                    "scph1000.bin", "scph5500.bin", "scph5502.bin"]
    )

    /// Nintendo DS, and the first system to arrive without any hardware-render work.
    ///
    /// THE FRAMEBUFFER IS BOTH SCREENS. melonDS emits 256x384, which is the two 256x192 screens
    /// already stacked top over bottom in one texture, so the existing composite draws them
    /// correctly with no change: the aspect ratio below is that whole stacked image, 2:3, not one
    /// screen's 4:3. Getting that wrong would letterbox the pair into the shape of a single screen
    /// and squash both.
    ///
    /// Software rendered on iOS, which is why this core is here at all. The plan in
    /// docs/SET_HW_RENDER_DESIGN.md put the DS behind ANGLE because melonDS HAS an OpenGL
    /// renderer, and its makefile never enables it for iOS: `HAVE_OPENGL := 0` is the default and
    /// only the unix block changes it. So the DS reaches the same pixel path the other five use.
    ///
    /// BIOS is the open question rather than graphics. melonDS traditionally wants bios7.bin,
    /// bios9.bin and firmware.bin, and newer builds can boot without them. The names are declared
    /// so the Settings BIOS row looks for them and reports what it finds, which is how a missing
    /// file becomes a readable note instead of a silent failure to boot.
    static let melonDS = CoreSpec(
        coreId: "melonds",
        displayName: "melonDS (Nintendo DS)",
        systems: ["ds"],
        library: "melonds_libretro_ios.dylib",
        width: 256, height: 384,
        maxWidth: 256, maxHeight: 384,
        aspectRatio: 256.0 / 384.0,
        fps: 59.8261,
        sampleRate: 32823,
        pixelFormat: 0,
        priority: 0,
        biosNames: ["bios7.bin", "bios9.bin", "firmware.bin"]
    )

    /// The TurboGrafx-16, on the lighter of mednafen's two PC Engine cores.
    ///
    /// "Fast" rather than the full Beetle PCE on purpose. Both play HuCard games identically as far
    /// as this app is concerned; the full core adds PC Engine CD and SuperGrafx, and CD needs a
    /// system card BIOS that cannot ship. So this covers exactly the part of the library that works
    /// with no files from the user.
    ///
    /// Every number here is from the core's own header: `MEDNAFEN_CORE_GEOMETRY_*`,
    /// `MEDNAFEN_CORE_TIMING_FPS` 59.82, a hardcoded 44100 sample rate, and RGB565, which is
    /// pixelFormat 0 in this app's encoding rather than libretro's. The 512 max width is real: the
    /// PC Engine can switch horizontal resolution mid-frame.
    static let mednafenPceFast = CoreSpec(
        coreId: "mednafen_pce_fast",
        displayName: "Beetle PCE Fast (TurboGrafx-16)",
        systems: ["tg16"],
        library: "mednafen_pce_fast_libretro_ios.dylib",
        width: 256, height: 243,
        maxWidth: 512, maxHeight: 243,
        aspectRatio: 6.0 / 5.0,
        fps: 59.82,
        sampleRate: 44100,
        pixelFormat: 0,
        priority: 0,
        biosNames: []
    )

    /// The Atari 2600, on the current upstream Stella.
    ///
    /// THE FIRST CORE IN THIS APP THAT NEEDS THE ROM IN MEMORY. Stella declares `need_fullpath`
    /// false and then `memcpy`s from `retro_game_info::data` with no path fallback, so the host's
    /// habit of handing every core a path and no bytes would have given it a zero-byte ROM. The
    /// engine now reads the file for any core that declares it wants bytes; see
    /// `NativeLibretroCore::load_content`.
    ///
    /// Geometry is from the core: 160 wide before its NTSC filter, and `AtariNTSC::outWidth(160)`
    /// = ((159 / 2) + 1) * 7 + 8 = 568 after it, which is the real maximum and has to be declared
    /// or a filtered frame would not fit. XRGB8888, which is pixelFormat 1 here. The sample rate is
    /// the core's own (262 * 76 * 60) / 38 = 31440.
    static let stella = CoreSpec(
        coreId: "stella2023",
        displayName: "Stella (Atari 2600)",
        systems: ["atari2600"],
        library: "stella2023_libretro_ios.dylib",
        width: 160, height: 210,
        maxWidth: 568, maxHeight: 312,
        aspectRatio: 4.0 / 3.0,
        fps: 60.0,
        sampleRate: 31440,
        pixelFormat: 1,
        priority: 0,
        biosNames: []
    )

    /// The Nintendo 64, on a software rasteriser and an interpreter.
    ///
    /// THE POINT OF THIS ENTRY IS THAT IT NEEDS NEITHER OF THE TWO THINGS THE N64 IS SUPPOSED TO
    /// NEED. parallel-n64's own iOS block sets `HAVE_OPENGL=0` and leaves `WITH_DYNAREC` empty, so
    /// it renders in software through the same path the other eight cores use and executes no
    /// generated code at all. No MoltenVK, and no `get-task-allow`.
    ///
    /// **It will be slow.** A software rasteriser plus an interpreter is slow on any phone, and
    /// that is the expected result rather than a fault to chase. What was in doubt was whether it
    /// runs, and this is what answers it.
    ///
    /// Geometry, aspect ratio and frame rate are ALL dynamic in this core: it sets them from the
    /// loaded ROM's region and its current resolution, and the engine re-reads them after load. The
    /// numbers here are only a seed for the first sizing, so 320x240 with a 640x480 ceiling is the
    /// honest declaration rather than a guess at one resolution. XRGB8888, which is pixelFormat 1
    /// in this app's encoding.
    static let parallelN64 = CoreSpec(
        coreId: "parallel_n64",
        displayName: "ParaLLEl N64 (software)",
        systems: ["n64"],
        library: "parallel_n64_libretro_ios.dylib",
        width: 320, height: 240,
        maxWidth: 640, maxHeight: 480,
        aspectRatio: 4.0 / 3.0,
        fps: 60.0,
        sampleRate: 48000,
        pixelFormat: 1,
        priority: 0,
        biosNames: []
    )

    /// Every core, in the order the HUD reports them.
    static let all: [CoreSpec] = [
        fceumm, snes9x, mgba, genesisPlusGx, pcsxReARMed, melonDS, mednafenPceFast, stella,
        parallelN64,
    ]

    static let byId: [String: CoreSpec] = Dictionary(
        uniqueKeysWithValues: all.map { ($0.coreId, $0) }
    )

    /// Where one file extension goes: which core runs it, and which system it is.
    ///
    /// The system is the part that was missing, and a core id could never have supplied it: mgba
    /// is GBA *and* GB *and* GBC, and genesis_plus_gx is Mega Drive *and* Master System *and* Game
    /// Gear. The on-screen pad has to know which of those it is drawing for, because a Game Gear
    /// pad has two buttons and a SNES pad has nine.
    struct Route: Sendable {
        let coreId: String
        let system: GameSystem
    }

    /// Lowercased file extension to its route. THE routing table, and still the only copy of it.
    ///
    /// Core ids come from the specs rather than from string literals so one cannot be misspelled on
    /// one side of the mapping.
    /// EVERY EXTENSION HERE WAS TAKEN FROM THE CORE'S OWN `valid_extensions` STRING, not from what
    /// a system is usually called. The six cores between them declare these:
    ///
    ///     fceumm            fds nes unf unif
    ///     snes9x            smc sfc swc fig bs st
    ///     mgba              gba gb gbc sgb
    ///     genesis_plus_gx   m3u mdx md smd gen bin cue iso chd bms sms gg sg 68k sgd
    ///     pcsx_rearmed      bin cue img mdf pbp toc cbn m3u chd iso exe
    ///     melonDS           nds ids dsi
    ///
    /// Reading that list is what turned up TWO WHOLE SYSTEMS this app already had the emulator for
    /// and was refusing to open: the Famicom Disk System through fceumm, and the SG-1000 through
    /// genesis_plus_gx. Several alternative file formats for systems already supported were being
    /// refused too, and `.smd` is the one that matters most in practice, because a Mega Drive ROM
    /// downloaded today is as likely to be interleaved `.smd` as plain `.md`.
    ///
    /// Not every declared extension is here, and each omission is a decision:
    ///
    ///   - `bin` is claimed by two cores AND is the companion file of a `.cue`. It stays unrouted
    ///     so a PlayStation track cannot be tapped as though it were a game. See `Route`.
    ///   - `cue`, `iso`, `chd` and `m3u` are claimed by genesis_plus_gx as well, for Sega CD. They
    ///     stay with the PlayStation, which is far more common and needs no BIOS to boot.
    ///   - `m3u` is NOT routed at all yet, even for the PlayStation. It is a playlist naming other
    ///     files, and the multi-file import only understands a cue sheet's tracks, so a multi-disc
    ///     game would import as one unopenable line. Worth doing; it is not free.
    ///   - `bs` and `st` are Satellaview and Sufami Turbo, which need a base cartridge to boot, and
    ///     `dsi` needs DSi firmware. An extension that always fails is worse than one that is
    ///     absent, because the first looks like a broken app.
    ///   - `exe` is a real PlayStation homebrew format and is deliberately skipped: a file called
    ///     `.exe` appearing as a tappable game invites someone to expect a Windows program to run.
    ///   - `mdx`, `68k`, `sgd`, `bms`, `img`, `cbn` and `ids` are rare enough that nobody will miss
    ///     them, and `img` is ambiguous enough to be anything at all.
    static let routeTable: [String: Route] = [
        "nes": Route(coreId: fceumm.coreId, system: .nes),
        // Unheadered NES dumps. Same core, same system, and refusing them was a gap rather than a
        // decision.
        "unf": Route(coreId: fceumm.coreId, system: .nes),
        "unif": Route(coreId: fceumm.coreId, system: .nes),
        // The Famicom's disk drive. A system in its own right; see `GameSystem.fds`. NEEDS the
        // Disk System BIOS, which is why `launch` checks for it by name and says so.
        "fds": Route(coreId: fceumm.coreId, system: .fds),
        "sfc": Route(coreId: snes9x.coreId, system: .snes),
        "smc": Route(coreId: snes9x.coreId, system: .snes),
        // Super Wild Card and Pro Fighter dumps: plain SNES ROMs in a different wrapper, which
        // snes9x reads directly.
        "swc": Route(coreId: snes9x.coreId, system: .snes),
        "fig": Route(coreId: snes9x.coreId, system: .snes),
        "gba": Route(coreId: mgba.coreId, system: .gba),
        "gb": Route(coreId: mgba.coreId, system: .gb),
        "gbc": Route(coreId: mgba.coreId, system: .gbc),
        // A Super Game Boy cartridge is a Game Boy game, so it is routed as one rather than given
        // a system of its own: the SNES border it would have drawn on real hardware is not
        // something mgba reproduces here.
        "sgb": Route(coreId: mgba.coreId, system: .gb),
        "sms": Route(coreId: genesisPlusGx.coreId, system: .sms),
        "gg": Route(coreId: genesisPlusGx.coreId, system: .gg),
        // Sega's first console, on the core that already supported it.
        "sg": Route(coreId: genesisPlusGx.coreId, system: .sg1000),
        "md": Route(coreId: genesisPlusGx.coreId, system: .genesis),
        "gen": Route(coreId: genesisPlusGx.coreId, system: .genesis),
        // Interleaved Mega Drive dumps, which are extremely common in older ROM sets and were
        // simply being turned away.
        "smd": Route(coreId: genesisPlusGx.coreId, system: .genesis),
        "cue": Route(coreId: pcsxReARMed.coreId, system: .ps1),
        "chd": Route(coreId: pcsxReARMed.coreId, system: .ps1),
        "pbp": Route(coreId: pcsxReARMed.coreId, system: .ps1),
        "iso": Route(coreId: pcsxReARMed.coreId, system: .ps1),
        // Two more single-file disc images pcsx_rearmed opens itself. Unlike a `.cue` these name
        // no companion track, so they need nothing from the multi-file import path.
        "mdf": Route(coreId: pcsxReARMed.coreId, system: .ps1),
        "toc": Route(coreId: pcsxReARMed.coreId, system: .ps1),
        "nds": Route(coreId: melonDS.coreId, system: .ds),
        // HuCard only. The core also declares cue, ccd, chd, toc and m3u for PC Engine CD, and
        // those are deliberately left out: CD games need a system card BIOS that cannot ship, so
        // routing them would offer a system that always fails. `.sgx` is absent because the FAST
        // core does not declare it; SuperGrafx needs the full Beetle PCE.
        "pce": Route(coreId: mednafenPceFast.coreId, system: .tg16),
        // `.a26` only. Stella also declares `.bin`, and `.bin` belongs to the PlayStation here as a
        // cue sheet's companion track, which must never be tappable. A 2600 ROM named `.bin` has to
        // be renamed, which is a real cost, and it is still the right trade: the alternative is
        // every PlayStation track appearing in the Library as an Atari game.
        "a26": Route(coreId: stella.coreId, system: .atari2600),
        // The three real N64 ROM formats. The core also declares `bin`, `zip`, `u1` and `ndd`:
        // `bin` is a PlayStation cue sheet's companion track here and must never be tappable,
        // `zip` would need archive handling this app does not have, and the other two are rare
        // enough that offering them would be inviting a failure rather than a system.
        "n64": Route(coreId: parallelN64.coreId, system: .n64),
        "z64": Route(coreId: parallelN64.coreId, system: .n64),
        "v64": Route(coreId: parallelN64.coreId, system: .n64),
    ]

    /// Extension to core id, DERIVED from the table above and never restated.
    ///
    /// Kept as its own property because everything that already routed a launch or labelled a
    /// Library row reads it, and because deriving it is what makes a second copy impossible: there
    /// is no edit that can add a system without also having a core, or a core without a system.
    static let routes: [String: String] = routeTable.mapValues { $0.coreId }

    /// A .bin is a CD track that a cue sheet names literally, never a launch target. It has to
    /// be importable so those references resolve, and it must never appear in the Library.
    static let trackExtension = "bin"

    /// Extensions the Library offers as a launch target: exactly the routing table's keys.
    ///
    /// DERIVED, AND IT USED NOT TO BE. This list and the one below were both written out by hand,
    /// which made them a second copy of the routing table, and the drift that invites had already
    /// happened silently in the other direction: routing an extension was not enough to make it
    /// work, because a file whose extension was missing from the hardcoded list was never copied in
    /// and so never appeared. Adding the Disk System, the SG-1000 and the alternative Mega Drive and
    /// SNES formats would have looked like routing them and changed nothing on screen.
    ///
    /// Sorted, because a `Dictionary`'s key order is not defined and this list is shown to the user
    /// in the Library's empty state and in the "nothing launchable" status line. An arbitrary order
    /// is acceptable there; one that reshuffles between launches is not.
    static let launchableExtensions: [String] = CoreCatalog.routeTable.keys.sorted()

    /// Extensions that may be copied into Documents.
    ///
    /// Wider than the launchable set, for two reasons that are both about files that are not games:
    ///
    ///   - a CD game is a set of files, so the .bin tracks must come in alongside the .cue that
    ///     names them. `bin` is importable and deliberately absent from `routeTable`, which is what
    ///     keeps a track out of the Library;
    ///   - firmware has to get into the visible folder before the install action can move it to the
    ///     one the cores read. That already worked for the PlayStation by accident, its BIOS being
    ///     a `.bin`, and did NOT work for `disksys.rom`: the picker refused the file outright, so
    ///     the Disk System's instructions could not be followed even in principle.
    ///
    /// Derived from the firmware list rather than restated, so adding a system whose firmware has a
    /// new extension does not need this line edited too.
    static let importableExtensions: [String] = {
        var seen = Set<String>()
        var names: [String] = []
        let firmwareExtensions = installableFirmwareNames.map {
            ($0 as NSString).pathExtension.lowercased()
        }
        for ext in launchableExtensions + [trackExtension] + firmwareExtensions
        where !ext.isEmpty && seen.insert(ext).inserted {
            names.append(ext)
        }
        return names
    }()

    /// Every BIOS filename any core in this build looks for, deduplicated, in declaration order.
    ///
    /// DERIVED from the specs rather than restated, for the same reason `routes` is derived from
    /// `routeTable`: a second list would drift, and a name that drifted would send the user hunting
    /// for a file the core is not looking for. Read by the Settings BIOS row and by the copy-across
    /// action, so both name exactly what the loader names.
    static let biosNames: [String] = {
        var seen = Set<String>()
        var names: [String] = []
        for spec in all {
            for name in spec.biosNames where seen.insert(name.lowercased()).inserted {
                names.append(name)
            }
        }
        return names
    }()

    /// Firmware a SYSTEM needs that is not declared against its core.
    ///
    /// One entry, and it exists because the two lists answer different questions. `CoreSpec`'s list
    /// drives the HUD's "BIOS (core):" line, so a name belongs there only if every game that core
    /// runs might want it. fceumm runs the NES, which needs nothing, and the Disk System, which
    /// cannot start without `disksys.rom`; putting that name on the core would put a missing-firmware
    /// line on every NES game, and leaving it off entirely meant the copy-across below did not
    /// recognise the file at all. So the user could follow the instructions exactly, drop the file
    /// in the visible folder, press the button, and be told nothing was found.
    ///
    /// The copy-across reads BOTH lists for that reason. Anything a system might need has to be
    /// here or the file can never reach the directory the cores actually read.
    static let extraFirmwareNames: [String] = ["disksys.rom"]

    /// Every firmware filename the install action will move across, from either list.
    static let installableFirmwareNames: [String] = {
        var seen = Set<String>()
        var names: [String] = []
        for name in biosNames + extraFirmwareNames where seen.insert(name.lowercased()).inserted {
            names.append(name)
        }
        return names
    }()

    /// "scph1001.bin, scph5501.bin, ..." for the lines that have to say what is recognised.
    ///
    /// Reports the INSTALLABLE set rather than the per-core one, because this string is shown next
    /// to the install button and has to name everything that button will act on.
    static func biosNameList() -> String {
        installableFirmwareNames.isEmpty ? "none" : installableFirmwareNames.joined(separator: ", ")
    }

    static func core(id: String) -> CoreSpec? {
        byId[id]
    }

    /// The core that will run a file with this extension, or nil when nothing is mapped.
    static func core(forExtension ext: String) -> CoreSpec? {
        guard let id = routes[ext.lowercased()] else { return nil }
        return byId[id]
    }

    /// Which system a file with this extension is, or nil when nothing is mapped.
    ///
    /// Read from the SAME table the core was resolved from, so the pad on screen and the core
    /// running it can never disagree about what console this is.
    static func system(forExtension ext: String) -> GameSystem? {
        routeTable[ext.lowercased()]?.system
    }

    /// What a Library row shows, so a wrong route is legible before anything is launched.
    static func routeLabel(forExtension ext: String) -> String {
        core(forExtension: ext)?.coreId ?? "no core"
    }

    /// ".nes, .sfc, .smc, ..." for the HUD lines that have to say what is accepted.
    static func extensionList(_ extensions: [String]) -> String {
        extensions.map { ".\($0)" }.joined(separator: ", ")
    }
}

// MARK: - One row of the Library

/// A game the Library can launch, as found in the app's own Documents directory.
///
/// `id` is the absolute path, which is stable across rescans and unique within a directory.
/// That matters: `List`/`ForEach` identity must never be an array index here, because the
/// array is rebuilt from disk after every import and every delete, and index identity would
/// animate the wrong rows and, worse, could launch the wrong file after a reorder.
struct LibraryEntry: Identifiable, Hashable, Sendable {
    /// The absolute path on disk, inside our sandbox. This is what the core is handed.
    let path: String
    /// The filename exactly as it sits on disk, extension included.
    let name: String
    /// The lowercased extension, already normalised for comparison and display.
    let ext: String
    /// The size of the GAME in bytes, or -1 when nothing about it could be read.
    ///
    /// For a cue sheet this is the sheet's own size PLUS the size of every unique track file it
    /// names that is on disk, because a sheet is a text file of a few dozen bytes and the tracks
    /// sitting beside it are the actual game: Crash Bandicoot is an 87-byte .cue next to a
    /// 632 MB .bin, and the 87 bytes are not what anyone means by the size of the game. For
    /// every other supported format, the cartridge ROMs and the self-contained disc images
    /// alike, it is simply that one file's own size.
    let byteCount: Int64
    /// What scanning the cue sheet for its tracks found, `.notApplicable` for a non-cue entry.
    let cueScan: CueScan
    /// When the file arrived, or nil when the filesystem would not say.
    ///
    /// Read so the library can order a "Recently added" shelf and pick a featured game
    /// DETERMINISTICALLY. That matters more than it sounds: the library array is rebuilt from disk
    /// after every import and every delete, so a hero chosen at random would change on every
    /// rescan. The modification date is used rather than the creation date because a copy into
    /// Documents sets both, and a file dropped in through the Files app is more reliably stamped
    /// with the former. It never takes part in identity, which is still the path alone.
    let addedAt: Date?

    var id: String { path }

    /// What a scan of a cue sheet's FILE lines found.
    ///
    /// Only a `.cue` is ever anything but `.notApplicable`: every other supported format is one
    /// self-contained file whose own size is the whole game. Each way the scan can come up short
    /// is its own case so the row can name it out loud, because "the tracks are not here" is the
    /// exact shape of the commonest import mistake, importing the .cue and leaving the .bin
    /// behind, and it is worth catching in the list rather than as a failed launch.
    enum CueScan: Hashable, Sendable {
        /// Not a cue sheet, so there was nothing to scan.
        case notApplicable
        /// A cue sheet whose own text could not be read, so its tracks could not be counted.
        case unreadable
        /// A cue sheet that was read but names no track file at all.
        case noTracksNamed
        /// A cue sheet naming `present + missing` unique track files.
        case tracks(present: Int, missing: Int)
    }

    init(url: URL) {
        path = url.path
        name = url.lastPathComponent
        let normalisedExtension = url.pathExtension.lowercased()
        ext = normalisedExtension
        let ownBytes = Self.fileSize(of: url)
        addedAt = Self.arrivalDate(of: url)

        // Every format but the cue sheet is one self-contained file, so its own size is the
        // game and there is nothing to add up.
        guard normalisedExtension == "cue" else {
            byteCount = ownBytes
            cueScan = .notApplicable
            return
        }

        guard let sheet = Self.cueText(at: url) else {
            // The sheet itself will not read, so the tracks behind it cannot be counted at all.
            // Its own size alone would read as a complete 87-byte game, which is why the row
            // says this instead of showing that number unqualified.
            byteCount = ownBytes
            cueScan = .unreadable
            return
        }

        let tracks = Self.trackURLs(
            inCueSheet: sheet,
            relativeTo: url.deletingLastPathComponent()
        )
        guard !tracks.isEmpty else {
            byteCount = ownBytes
            cueScan = .noTracksNamed
            return
        }

        // Add up what is actually on disk and count what is not. Each track is only ever
        // stat'ed, never opened.
        var total: Int64 = 0
        var readAnything = false
        var present = 0
        var missing = 0
        if ownBytes >= 0 {
            total = ownBytes
            readAnything = true
        }
        for track in tracks {
            let size = Self.fileSize(of: track)
            if size >= 0 {
                total += size
                present += 1
                readAnything = true
            } else {
                missing += 1
            }
        }

        // -1, and so "size unknown", only when not one single file could be measured. A total
        // built from some of them is still worth showing, and the missing count says what it
        // is missing.
        byteCount = readAnything ? total : -1
        cueScan = .tracks(present: present, missing: missing)
    }

    /// Human-sized size. Hand-rolled rather than routed through a formatter because this is a
    /// diagnostic read-out: it must not be localised and it must not vary by locale.
    var sizeText: String {
        if byteCount < 0 { return "size unknown" }
        let megabytes = Double(byteCount) / (1024.0 * 1024.0)
        if megabytes >= 1.0 {
            return String(format: "%.1f MB", megabytes)
        }
        let kilobytes = Double(byteCount) / 1024.0
        if kilobytes >= 1.0 {
            return String(format: "%.0f KB", kilobytes)
        }
        // Rounding a sub-kilobyte file to "0 KB" reads as an empty or broken import when the
        // file is perfectly fine, so the real byte count goes out instead. A cue sheet with its
        // tracks present no longer lands here, because its size is now the whole game, but a
        // sheet whose tracks are missing still does, and a bare 87 bytes next to the missing
        // note is exactly the read-out that case deserves.
        return "\(byteCount) bytes"
    }

    /// The secondary row line, built as a `String` so `Text` takes its verbatim initialiser.
    ///
    /// It names the core the row will launch on, read from the same `CoreCatalog.routes` the
    /// launch path uses. That is deliberate: a routing mistake shows up in the list, before a
    /// tap, instead of as a game that boots on the wrong emulator. Whatever else gets inserted,
    /// the core name stays LAST, so that affordance is where it has always been.
    var detail: String {
        var parts = [ext.uppercased(), sizeText]
        parts.append(contentsOf: cueNotes)
        parts.append(CoreCatalog.routeLabel(forExtension: ext))
        return parts.joined(separator: " · ")
    }

    /// The cue-specific notes that sit between the size and the core name.
    ///
    /// Empty for a format that is one file, and empty for a cue sheet whose every track was
    /// found: a healthy row reads exactly as it did before. Anything else is spelled out, since
    /// a size that quietly excludes a track the game needs is worse than no size at all.
    var cueNotes: [String] {
        switch cueScan {
        case .notApplicable:
            return []
        case .unreadable:
            return ["cue sheet unreadable, tracks not counted"]
        case .noTracksNamed:
            return ["cue names no track file"]
        case let .tracks(_, missing):
            if missing < 1 { return [] }
            return [missing == 1 ? "1 track missing" : "\(missing) tracks missing"]
        }
    }

    // MARK: Measuring a game that is more than one file

    /// The size of one file in bytes, or -1 when it could not be read.
    ///
    /// Stats the file and never opens it. That is not an optimisation, it is the constraint: a
    /// PlayStation track is hundreds of megabytes, and reading one just to measure it would
    /// stall the UI on every rescan and could be fatal on memory. The only file this type ever
    /// reads the contents of is the cue sheet, which is well under a kilobyte.
    private static func fileSize(of url: URL) -> Int64 {
        guard let values = try? url.resourceValues(forKeys: [.fileSizeKey]),
              let size = values.fileSize
        else { return -1 }
        return Int64(size)
    }

    /// When the file arrived, modification date first and creation date as the fallback.
    ///
    /// Stats only, like `fileSize`, so a rescan of a library of PlayStation discs still opens
    /// nothing. Nil is a perfectly good answer: the shelf and the hero both fall back to name
    /// order, which is stable for the same reason a date is.
    private static func arrivalDate(of url: URL) -> Date? {
        guard let values = try? url.resourceValues(forKeys: [.contentModificationDateKey,
                                                            .creationDateKey])
        else { return nil }
        return values.contentModificationDate ?? values.creationDate
    }

    /// The text of a cue sheet, or nil when it cannot be read at all.
    ///
    /// UTF-8 first, then Latin-1, because a sheet written by an older Windows ripper is not
    /// always UTF-8 and a single stray high byte would otherwise make the whole sheet
    /// unreadable. Latin-1 cannot fail on any byte sequence, so nil here means the read itself
    /// failed, not that the text was merely odd.
    private static func cueText(at url: URL) -> String? {
        if let utf8 = try? String(contentsOf: url, encoding: .utf8) {
            return utf8
        }
        return try? String(contentsOf: url, encoding: .isoLatin1)
    }

    /// Every unique track file a cue sheet names, resolved against the sheet's own directory.
    ///
    /// Resolution is relative to the SHEET, not to Documents. Imports land flat today so the two
    /// are the same directory, but the filenames in a sheet are written relative to the sheet,
    /// and resolving them that way is what stays correct if that ever changes.
    ///
    /// Deduplicated by resolved path, and that is not a nicety: a sheet may name the same .bin
    /// on more than one FILE line, and counting it twice would report a 632 MB game as 1.2 GB.
    /// A genuine multi-track dump names several DIFFERENT files, and every one of those counts.
    static func trackURLs(inCueSheet text: String, relativeTo directory: URL) -> [URL] {
        var seen = Set<String>()
        var tracks: [URL] = []

        // Splitting on `isNewline` handles CRLF as well as LF, because Swift treats "\r\n" as
        // one grapheme, and cue sheets are very often CRLF.
        for line in text.split(whereSeparator: { $0.isNewline }) {
            // Every line is offered, and anything that does not name a file comes back nil:
            // TRACK, INDEX and PREGAP name none, and a malformed FILE line is skipped rather
            // than guessed at. A half-written sheet must not take the whole Library down.
            guard let filename = cueFilename(inCueLine: String(line)) else { continue }

            let resolved = directory.appendingPathComponent(filename).standardizedFileURL
            // Keyed on the RESOLVED path rather than the name as written, so two spellings of
            // one file cannot both be counted.
            if seen.insert(resolved.path).inserted {
                tracks.append(resolved)
            }
        }

        return tracks
    }

    /// The track filename a single cue-sheet line names, or nil when it names none.
    ///
    /// Pure and self-contained, which is what lets the cases that cannot be reproduced on a
    /// device, a quoted name with spaces, an unquoted one, a lone quote, each be reasoned about
    /// in isolation. The FILE test lives here rather than in the caller so there is exactly one
    /// of it, and so that handing this function any other line is safe.
    static func cueFilename(inCueLine line: String) -> String? {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        let tokens = trimmed.split(whereSeparator: { $0.isWhitespace })
        // Case-insensitive on the first token: sheets in the wild write FILE, and some rippers
        // write file.
        guard let keyword = tokens.first, keyword.lowercased() == "file" else { return nil }

        // Normally quoted, and that is the case that matters. Everything between the FIRST quote
        // and the LAST quote on the line, which is what keeps a name with spaces in it whole:
        //     FILE "Crash Bandicoot (USA).bin" BINARY
        if let firstQuote = trimmed.firstIndex(of: "\"") {
            guard let lastQuote = trimmed.lastIndex(of: "\""), firstQuote < lastQuote else {
                // A single lone quote. Anything read out of that would be a guess, so the line
                // is skipped rather than turned into a filename that was never written.
                return nil
            }
            let quoted = trimmed[trimmed.index(after: firstQuote)..<lastQuote]
                .trimmingCharacters(in: .whitespaces)
            return quoted.isEmpty ? nil : quoted
        }

        // Unquoted: the name is what sits between FILE and the trailing format keyword, which is
        // one of BINARY, MOTOROLA, AIFF, WAVE or MP3. So the last token is dropped and the rest
        // rejoined, which keeps an unquoted name with spaces in it intact:
        //     FILE my game.bin BINARY  ->  my game.bin
        // Fewer than three tokens means there is no name between the keyword and the format, so
        // there is nothing to take.
        guard tokens.count >= 3 else { return nil }
        let filename = tokens.dropFirst().dropLast().joined(separator: " ")
        return filename.isEmpty ? nil : filename
    }
}

// MARK: - The engine, owned once

/// Holds the engine for the app's lifetime.
///
/// One instance, created once. The engine is `Send + Sync` on the Rust side, it is a
/// `Mutex<EmulatorBridge>`, so it is safe to reach from any actor, which is exactly why
/// `RefCell` was not an option there.
@MainActor
final class EngineHost: ObservableObject {
    let engine: ContinuumEngine

    /// Fit, filter, fast-forward multiplier, volume and the rewind budget.
    ///
    /// Its own object rather than more properties here, following `artwork`: it owns its own
    /// storage keys and defaults, and re-asserts them into an engine whose pacer and frame
    /// target do not survive a session. See `EmulationSettings`.
    let emulation: EmulationSettings

    @Published var frameCount: UInt64 = 0
    @Published var displayFps: Double = 0
    @Published var dropped: UInt32 = 0
    @Published var status: String = "waiting for the surface"
    @Published var gpu: String = ""
    /// The on-screen BIOS/HLE line. Written when the cores are declared at attach, and
    /// refreshed whenever a core loads, so a missing BIOS is a legible condition BEFORE a game
    /// is tapped rather than a silent drop in compatibility afterwards. Empty for a core that
    /// needs no BIOS, which is every core here except PCSX ReARMed.
    @Published var bios: String = ""
    /// The on-screen core line: how many of the five were declared, and which dylibs are not
    /// in the bundle.
    ///
    /// Written once when the surface attaches. Its whole job is to answer "is the core even
    /// shipped" before the user taps a game, because a dylib that failed to embed and a core
    /// that fails to load look identical from the Library otherwise.
    @Published var cores: String = ""
    /// True once a ROM has been launched. Drives the HUD affordance label and re-entrancy.
    @Published var running = false

    /// The game currently on screen, or nil when the Library is showing.
    ///
    /// This is what switches between the two screens, and it is the ENTRY rather than a `Bool` on
    /// purpose: the player screen needs the filename for its title bar and the extension to decide
    /// which pad to draw, and holding the entry means neither has to be looked up a second time or
    /// stored twice. Set only on `launch`'s success path, cleared only in `stopSession`.
    @Published var activeEntry: LibraryEntry?

    /// Whether the engine is paused. Kept in step with `engine.pause()` and `engine.resume(...)`
    /// so the control can show which of the two it will do next.
    @Published var paused = false

    /// Whether the full diagnostic block is on screen.
    ///
    /// The block itself is unchanged and nothing in it was dropped; it simply stopped being
    /// permanently in front of the game. The thin status line above it is always visible, so a
    /// failure is still legible with this off.
    @Published var showDiagnostics = false

    /// What the SAFE half of the JIT probe found, read once at startup.
    ///
    /// Safe means it maps an executable page and releases it without writing to it or running it.
    /// The half that runs code is behind a button in Settings, because it can get the app killed
    /// and used to do that ON LAUNCH: see `EmulatorBridge.jitProbe` and `jitProbeExecution`.
    ///
    /// Read ONCE rather than on demand, and kept, because the answer cannot change while the app
    /// is running: it is a property of the installed binary's entitlements and the signature that
    /// carried them. Probing repeatedly would map and unmap a page for an answer already known.
    ///
    /// It is here at all because it decides whether N64 is possible. An N64 interpreter is far too
    /// slow to be playable, so every usable N64 core needs a recompiler, and a recompiler needs to
    /// be able to write instructions into memory and jump to them. The entitlements have been in
    /// the build since the beginning and had never been exercised.
    @Published var jitLine: String = ""

    /// Whether MoltenVK loaded and answered, read once at startup.
    ///
    /// The gate on every system that renders through a GPU rather than in software: Dreamcast, PSP,
    /// the 3DS, and an N64 that runs at a playable speed. All four are unreachable while the app can
    /// only show pixels a core rasterised on the CPU. See `vulkan_probe.rs`.
    @Published var vulkanLine: String = ""

    /// The on-screen pad's layout: where the two thumb clusters sit, how big they are and how
    /// faint. One value, which is why the editor turned out to be a screen that writes six numbers
    /// rather than a rewrite. See `TouchLayout` and `TouchLayoutEditor`.
    ///
    /// Persisted on every change rather than on a later flush, for the reason `toggleFavourite`
    /// gives: a sideloaded build can be killed by the OS at any moment, and an arrangement that did
    /// not survive that would read as the editor not working. The write is cheap because the editor
    /// only assigns here when a drag or a slider is RELEASED, never per frame of one; see
    /// `TouchControlsView.onLayoutEdited`.
    ///
    /// Stored sanitised, and read back through `TouchLayout.restored(from:)` which sanitises again.
    /// Twice on purpose: this key outlives the build that wrote it, so the limits it was written
    /// against are not necessarily the limits it will be read against.
    @Published var touchLayout: TouchLayout = .standard {
        didSet {
            guard oldValue != touchLayout else { return }
            guard let encoded = touchLayout.storedRepresentation else {
                // Nothing is written on a failed encode, rather than the key being cleared. The
                // layout the user can see on screen is still the live one, and clearing would
                // silently reset it on the next launch with nothing to explain why.
                status = "could not save the control layout; it still applies to this session"
                return
            }
            UserDefaults.standard.set(encoded, forKey: Self.touchLayoutKey)
        }
    }

    /// Whatever the control surface last had to report, which today is only a layout overlap.
    @Published var controlNote: String = ""

    /// The rect the picture may draw into without a control sitting on it, published by the
    /// control surface after every layout. Nil means the whole screen, which is what the Library
    /// wants and what the player falls back to before the first layout pass.
    @Published var pictureArea: CGRect?

    /// Queued audio frames and underruns in the ENGINE's ring, from the last tick.
    ///
    /// These are only half the latency now. `drain_audio` is exported and the display link
    /// empties this ring into `audio`'s every frame, so the engine side sits near zero in steady
    /// state and the device side carries the buffer. Both halves are added up in `audioLine`,
    /// because what a player hears is the sum.
    @Published var audioQueued: UInt32 = 0
    @Published var audioUnderruns: UInt32 = 0

    /// The device audio path: the session, the graph, and the lock-free ring between the display
    /// link and the render block.
    ///
    /// Owned here for the app's lifetime, for the same reason `padInput` is: SwiftUI rebuilds
    /// views freely, and neither the display link nor a live `AVAudioEngine` may be left holding
    /// an object that was replaced underneath them. Built with the SAME engine instance, because
    /// the whole design rests on the drain happening on the display link's thread.
    let audio: AudioOutput

    /// Mirrored from `audio` once per frame so the HUD can read it without touching the audio
    /// object from a view body. Every one of these is measured rather than assumed.
    @Published var audioRunning = false
    @Published var audioStatus = "audio: not started"
    @Published var audioDeviceRate: Double = 0
    @Published var audioDeviceBuffered: Int = 0
    @Published var audioRenderUnderruns: Int = 0
    @Published var audioRenderedFrames: Int = 0
    @Published var audioDroppedFrames: Int = 0

    /// The engine's own view of its ring, polled once a second rather than once a frame.
    ///
    /// `engine.audioStats()` takes the engine mutex, and the telemetry strip re-renders on every
    /// tick, so reading it per frame would take that lock sixty times a second on the same thread
    /// that already holds it for the whole of each tick. Once a second is plenty for numbers that
    /// only matter when they are non-zero.
    @Published var audioOverruns: UInt64 = 0
    @Published var audioSourceRate: UInt32 = 0
    @Published var audioCapacityFrames: UInt32 = 0
    private var audioStatsCountdown = 0

    /// The live read-through the render loop uses to fetch pad state.
    ///
    /// Owned here, for the app's lifetime, rather than by the player screen, because SwiftUI
    /// rebuilds views freely and the display link must never be left holding a box that was
    /// replaced. The box is a plain reference type with a weak link to whichever control surface is
    /// on screen, so "no controls" is a released pad rather than a missing value.
    let padInput = PadInputSource()

    /// Physical controllers: MFi, Xbox, DualShock and DualSense, through GameController.
    ///
    /// Owned here for the app's lifetime for the same two reasons `padInput` and `audio` are. It
    /// registers notification observers that must not be torn down and rebuilt by a SwiftUI view
    /// update, and the display link polls it every frame, so it may not be an object that a view
    /// rebuild can replace underneath the tick.
    ///
    /// Its own object rather than more properties here, following `EmulationSettings`: it owns one
    /// stored preference with its own key, and the Settings and player screens observe it directly
    /// so a pad being plugged in changes what is on screen without this type mirroring every field.
    let controllers: PhysicalControllers

    /// The launchable games found in Documents, newest scan wins.
    @Published var library: [LibraryEntry] = []
    /// The Library's own status line, kept separate from `status` on purpose.
    ///
    /// An import writes its summary to `status`, and the rescan that follows must not wipe
    /// that summary out. Two independent lines mean the user reads both the import result and
    /// the state of the Library at the same time, which is the whole point of a HUD that is
    /// also the only debugger on a sideloaded build.
    @Published var libraryStatus: String = "library: not scanned yet"

    // MARK: The library shell's own state

    /// Cover art: the catalogue, the fetching, the disk store and the negative cache.
    ///
    /// Owned here, once, for the app's lifetime. That is what keeps the tier 5 picker's delegate
    /// alive long enough to be called back, for exactly the reason `importPickerDelegate` below
    /// exists.
    let artwork = ArtworkStore()

    /// The artwork read-out, mirrored from the store so the diagnostics panel can show it without
    /// observing a second object. Never empty.
    @Published var artworkLine: String = "artwork: nothing looked up yet"

    /// The game whose detail sheet is open, or nil. The sheet is presented from this, so setting it
    /// to nil is what closes it.
    @Published var detailEntry: LibraryEntry?

    /// Which library tab is showing, and what is in the search field.
    ///
    /// HELD HERE RATHER THAN AS VIEW STATE, because the shell is removed from the view tree while a
    /// game is running: `RootView` shows either the shell or the player, which is what keeps the
    /// one Metal canvas mounted underneath both. View state would be discarded with it, so leaving
    /// a game would drop the user back on Home with their search cleared, having started from
    /// Favorites.
    @Published var libraryTab: LibraryTab = .home
    @Published var librarySearch: String = ""

    /// The favourited games, keyed on the SAME identity `LibraryEntry` uses: the absolute path.
    ///
    /// Not an array index, because the library array is rebuilt from disk after every import and
    /// delete. Ids are never pruned on a rescan either: a file that is temporarily absent, because
    /// it is being replaced or because the Files app is mid-copy, is not a reason to forget it was
    /// a favourite. `favouriteEntries` simply shows the ones that are currently there.
    @Published private(set) var favourites: Set<String> = []

    /// How All Games arranges itself, and whether Home carries a shelf per system.
    ///
    /// Both persist, and both are about the LIBRARY. Moving the on-screen game controls is a
    /// different setting with a confusingly similar name: it lives in `touchLayout` above.
    @Published var libraryLayout: LibraryLayout = .grid {
        didSet {
            guard oldValue != libraryLayout else { return }
            UserDefaults.standard.set(libraryLayout.rawValue, forKey: Self.layoutKey)
        }
    }

    @Published var showsSystemShelves: Bool = true {
        didSet {
            guard oldValue != showsSystemShelves else { return }
            UserDefaults.standard.set(showsSystemShelves, forKey: Self.systemShelvesKey)
        }
    }

    private static let favouritesKey = "continuum.favourites.v1"
    private static let layoutKey = "continuum.library.layout.v1"
    private static let systemShelvesKey = "continuum.library.systemShelves.v1"
    /// Named for the GAME controls, not for the library layout `layoutKey` holds. The two are
    /// unrelated settings with confusingly similar names, and a key that did not say which it meant
    /// would be the first thing a later reader got wrong.
    private static let touchLayoutKey = "continuum.controls.touchLayout.v1"

    /// The favourites that are on disk right now, in the library's own order.
    var favouriteEntries: [LibraryEntry] {
        library.filter { favourites.contains($0.id) }
    }

    /// Adds or removes a favourite and persists the set immediately.
    ///
    /// Immediately rather than on some later flush, because a sideloaded build can be killed by the
    /// OS at any moment and a favourite that did not survive would look like the feature not
    /// working.
    func toggleFavourite(_ entry: LibraryEntry) {
        if favourites.contains(entry.id) {
            favourites.remove(entry.id)
            status = "removed \(entry.name) from favorites"
        } else {
            favourites.insert(entry.id)
            status = "added \(entry.name) to favorites"
        }
        UserDefaults.standard.set(Array(favourites), forKey: Self.favouritesKey)
    }

    /// The indices in `library` of the entries with these ids.
    ///
    /// The bridge between a filtered view and `deleteEntries(at:)`, which indexes the real array. A
    /// filtered list's offsets are NOT the library's offsets, and handing them over directly would
    /// delete a different game. An id that is no longer in the library contributes nothing, and
    /// `deleteEntries` already reports an empty selection as its own condition.
    func indices(matching ids: Set<String>) -> IndexSet {
        IndexSet(library.indices.filter { ids.contains(library[$0].id) })
    }

    // MARK: What the engine says about itself

    /// The core the engine reports as resident, if any, with the state it reported.
    ///
    /// ASKED OF `engine.coreState` EVERY TIME, never cached. That is the same rule
    /// `ensureCoreLoaded` follows and for the same reason: `engine.stop()` unloads the core behind
    /// Swift's back under the Drop retention policy, so any Swift flag remembering residency is
    /// wrong the moment a session ends.
    func residentCore() -> (spec: CoreSpec, state: String)? {
        for spec in CoreCatalog.all {
            let state = engine.coreState(coreId: spec.coreId) ?? "unknown"
            if Self.usableCoreStates.contains(state) {
                return (spec: spec, state: state)
            }
        }
        return nil
    }

    /// The options the resident core declared about itself, read fresh.
    ///
    /// Empty when no core is resident, which is the normal state while the library is on screen:
    /// leaving a game hands the core back to the registry and it is unloaded. Settings says so
    /// rather than showing an empty list with no explanation.
    func coreOptionRecords() -> [CoreOptionRecord] {
        guard residentCore() != nil else { return [] }
        return engine.coreOptions()
    }

    /// Runs the half of the JIT probe that executes code, on request only.
    ///
    /// Writes the result into `jitLine` so it lands in the same place the safe answer did, and into
    /// the status line so it is visible without opening the diagnostics panel.
    ///
    /// IF THE APP CLOSES DURING THIS, THAT IS THE ANSWER. The kernel refuses a forbidden execute by
    /// terminating the process, so there is nothing to catch and nothing to report; reopening the
    /// app is safe and nothing is lost. The button that calls this says so.
    func runJitExecutionProbe() {
        status = "running the JIT test; if the app closes, that IS the result and reopening is safe"
        let line = engine.jitProbeExecution()
        jitLine = line
        status = line
    }

    /// Sets one core option, and names the outcome either way.
    func applyCoreOption(key: String, value: String, label: String) {
        let shown = label.isEmpty ? key : label
        do {
            try engine.setCoreOption(key: key, value: value)
            status = "core option \(shown) set to \(value)"
        } catch {
            status = "core option \(shown) could not be set to \(value): \(error)"
        }
    }

    /// Copies any recognised BIOS file out of Documents and into the directory the cores read.
    ///
    /// THIS EXISTS BECAUSE THE TWO DIRECTORIES ARE NOT THE SAME ONE, which is easy to miss and
    /// impossible to diagnose from the outside. `UIFileSharingEnabled` exposes DOCUMENTS as the
    /// Continuum folder in the Files app, and that is where an import lands, but the system
    /// directory handed to a core through GET_SYSTEM_DIRECTORY is APPLICATION SUPPORT, which the
    /// Files app does not show at all. So a user can put scph1001.bin somewhere perfectly sensible
    /// and the core will never see it. This is the one step across.
    ///
    /// Copies rather than moves, so the file the user put in the visible folder stays where they
    /// can see it. Overwrites, because re-installing a BIOS is a normal thing to do. Every outcome
    /// gets its own line, including the two that are conditions rather than errors.
    func installBiosFromDocuments() {
        guard let documents = documentsDirectory() else {
            status = "BIOS install failed: no Documents directory"
            return
        }
        guard let systemDir = systemDirectory() else {
            status = "BIOS install failed: no Application Support directory for the core to "
                + "read from"
            return
        }

        var installed: [String] = []
        var failures: [String] = []

        for name in CoreCatalog.installableFirmwareNames {
            // Matched case-insensitively on the name the LOADER looks for, then copied under that
            // exact spelling, because the core opens the name it declared and iOS filesystems are
            // case-preserving. A file called SCPH1001.BIN is the right file with the wrong case.
            let source = documents.appendingPathComponent(name)
            var resolved: URL?
            if FileManager.default.fileExists(atPath: source.path) {
                resolved = source
            } else if let contents = try? FileManager.default.contentsOfDirectory(
                at: documents,
                includingPropertiesForKeys: nil,
                options: [.skipsHiddenFiles]
            ) {
                resolved = contents.first {
                    $0.lastPathComponent.lowercased() == name.lowercased()
                }
            }
            guard let resolved else { continue }

            let destination = systemDir.appendingPathComponent(name)
            do {
                if FileManager.default.fileExists(atPath: destination.path) {
                    try FileManager.default.removeItem(at: destination)
                }
                try FileManager.default.copyItem(at: resolved, to: destination)
                installed.append(name)
            } catch {
                failures.append("\(name): \(error.localizedDescription)")
            }
        }

        if installed.isEmpty && failures.isEmpty {
            status = "no BIOS file found in the Continuum folder; the recognised names are "
                + CoreCatalog.biosNameList()
            return
        }
        if failures.isEmpty {
            status = "installed BIOS: \(Self.nameList(installed)); relaunch the game to use it"
        } else if installed.isEmpty {
            status = "BIOS install failed: \(Self.nameList(failures))"
        } else {
            status = "installed BIOS: \(Self.nameList(installed)); "
                + "failed: \(Self.nameList(failures))"
        }

        // Refresh the read-out so the BIOS line reflects what was just copied rather than what was
        // true at attach.
        if let biosCore = CoreCatalog.all.first(where: { !$0.biosNames.isEmpty }) {
            bios = biosStatus(for: biosCore, in: systemDir)
        }
    }

    /// The import picker's delegate, retained here for the app's lifetime.
    ///
    /// THIS PROPERTY IS A FIX, NOT A STYLE CHOICE. `UIDocumentPickerViewController.delegate`
    /// is a WEAK reference, so the delegate object must be owned by something else for as long
    /// as the picker is on screen. A delegate created as a local inside the presenting function
    /// is released the moment that function returns, the picker's weak reference goes nil, and
    /// the callback never fires: the sheet then dismisses and nothing happens. This host is
    /// owned by a `@StateObject` on the root view of the only `WindowGroup`, so it lives as
    /// long as the app does, and this strong reference therefore provably outlives any picker
    /// it is handed to.
    private lazy var importPickerDelegate = ImportPickerDelegate(host: self)

    /// The picker that is currently on screen, retained for as long as it is up.
    ///
    /// UIKit owns a presented view controller, so this is belt and braces against the one
    /// remaining invisible failure: if anything releases or tears the picker down early, it
    /// disappears without a word and its delegate is never called, which reads on the HUD
    /// exactly like a delegate that refuses to fire. Holding a strong reference here means an
    /// early release cannot be what happened, so the HUD's silence has to mean something else.
    /// Cleared through `releaseActivePicker()` the moment any callback reports back.
    private var activePicker: UIDocumentPickerViewController?

    /// The content types the picker offers: `public.item`, which every file conforms to.
    ///
    /// DO NOT "TIGHTEN" THIS INTO A LIST OF ROM TYPES. It was learned the hard way, twice, and
    /// shipping four more cores made it worse rather than better: iOS ships no built-in
    /// `UTType` for .nes, .sfc, .smc, .gba, .sms, .md, .gen, .gg, .cue, .bin, .chd or .pbp, so
    /// `UTType(filenameExtension: "cue")` and friends return nil, and a picker whose
    /// `allowedContentTypes` is built from them GREYS THOSE EXACT FILES OUT: the user opens
    /// the picker, sees their game, and cannot tap it. Declaring the types properly would mean
    /// exporting a dozen custom UTIs from the Info.plist, which is more moving parts than a
    /// sideloaded build needs.
    ///
    /// So the picker stays permissive and NOTHING is greyed out, and acceptance is enforced
    /// AFTER the pick, by extension, in `importFiles`. Rejected files are named on the HUD
    /// along with their extension, so a wrong pick is legible rather than mysterious.
    /// Directories nominally conform to `public.item` too; one picked by accident has no
    /// importable extension and is reported by that same rejection path.
    private static let pickerContentTypes: [UTType] = [UTType.item]

    /// How many filenames the HUD names before it starts counting instead. Enough to identify
    /// what happened, short enough to stay on screen.
    private static let reportedNameLimit = 6

    /// Save states: the index, the compatibility gate, the auto-save and the resume.
    ///
    /// A separate object for the same reason `EmulationSettings` is one, and it is `let` rather than
    /// `@Published` because the object never changes; the views that need it observe it directly.
    let saveStates: SaveStates

    /// The per-game cheat lists, and the one path that pushes them into a core.
    let cheats: CheatStore

    init() {
        engine = ContinuumEngine()
        // Built here, with that engine, and never rebuilt. The graph itself is not started until
        // a game launches: holding an active `AVAudioSession` while the user browses their
        // library would duck whatever music they had playing for no reason at all.
        audio = AudioOutput(engine: engine)
        // Reads its own stored preferences and pushes them into the engine as it is built, so
        // the first frame of the first game already looks and sounds the way the user left it.
        emulation = EmulationSettings(engine: engine)
        // One page mapped and unmapped, nothing written to it and nothing run from it. Done here so
        // the answer is on the HUD before any game is launched, because it has to be readable
        // without a core running.
        //
        // THE MAPPING-ONLY PROBE, AND THIS LINE IS WHY THE APP OPENS AT ALL. What used to be here
        // wrote a function into a page and called it. iOS terminates a process for executing a page
        // it just wrote unless the dynamic-codesigning entitlement is genuinely in force, and
        // whether it is depends on how this copy was signed and installed rather than on anything
        // in the build. So every install whose signature did not carry it opened and was killed on
        // this line, before drawing anything, including the line the probe existed to print. The
        // half that runs code is a button in Settings now.
        jitLine = engine.jitProbe()
        // Step 3 of the graphics road, and the question everything above software rendering waits
        // on. Safe here for the same reason the line above now is: a dlopen and two reads, with
        // every failure arriving as a string rather than as a dead app.
        vulkanLine = engine.vulkanProbe(
            frameworksDir: Bundle.main.privateFrameworksPath ?? ""
        )
        // Scans for already-paired controllers as it is built, because a pad connected before the
        // app launched has already sent its connect notification to nobody. Given the engine so
        // that a disconnect can release the gamepad input layer immediately, which is not something
        // that can wait for a frame: a pad unplugged mid-press has no further poll coming, so
        // whatever it was holding would stay held for the rest of the session.
        controllers = PhysicalControllers(engine: engine)
        // Reads its metadata index as it is built, so the library can tell which games have an
        // auto-save before anything is launched, and registers the two notification observers that
        // write the auto-save when the app stops being the thing in front of the user.
        saveStates = SaveStates(engine: engine)
        // Reads the stored cheat lists as it is built. Nothing is pushed to a core here: a cheat
        // table belongs to a session, so the push happens on launch.
        cheats = CheatStore(engine: engine)

        // The remembered preferences, read before anything can display. Each one falls back to its
        // default rather than to nil, so a first launch and a corrupted value behave the same way.
        let defaults = UserDefaults.standard
        favourites = Set(defaults.stringArray(forKey: Self.favouritesKey) ?? [])
        if let stored = defaults.string(forKey: Self.layoutKey),
           let layout = LibraryLayout(rawValue: stored) {
            libraryLayout = layout
        }
        if let stored = defaults.object(forKey: Self.systemShelvesKey) as? Bool {
            showsSystemShelves = stored
        }
        // Assigned unconditionally, because `restored(from:)` already answers absent, unreadable
        // and out of range with the default. The extra `didSet` write the other preferences avoid
        // by testing first does not happen here either: `didSet` does not fire for an assignment
        // made inside `init`, which is also why this cannot loop back through the encode above.
        touchLayout = TouchLayout.restored(from: defaults.data(forKey: Self.touchLayoutKey))

        // Scan up front so the Library is populated even if the Metal attach later fails.
        // An attach failure must not also hide the games the user already imported.
        refreshLibrary()

        // Wired last, because it hands the store a reference to a fully initialised host. The store
        // writes its read-out through `artworkLine`, and a network failure through `status`, so an
        // artwork problem is legible on the strip rather than only behind the Settings tab.
        artwork.attach(host: self)

        // Same reason, and these two need more from the host than the artwork store does: which
        // game is running, which only the launch path knows, and the status line, which is where a
        // refused load or a failed auto-save has to be said out loud.
        saveStates.attach(host: self)
        cheats.attach(host: self)

        // Wired after `init` has finished with `self`, for the same reason. A pad connecting or
        // disconnecting is exactly the kind of thing the always-visible status line is for: it is
        // the one line a player reads when a controller does nothing, and it says whether the app
        // saw the pad at all. Nil during the scan above, deliberately, so a pad already attached at
        // launch cannot overwrite the surface-attach message with a line about itself.
        controllers.onChange = { [weak self] line in
            self?.status = line
        }
    }

    // MARK: Directories

    /// Where the core reads its BIOS and writes its savedata. Created if absent so the core
    /// always has a real, writable directory to hand back through GET_SYSTEM_DIRECTORY.
    private func systemDirectory() -> URL? {
        guard let dir = FileManager.default.urls(for: .applicationSupportDirectory,
                                                 in: .userDomainMask).first else {
            return nil
        }
        if !FileManager.default.fileExists(atPath: dir.path) {
            try? FileManager.default.createDirectory(at: dir,
                                                     withIntermediateDirectories: true)
        }
        return dir
    }

    /// The app's own Documents directory: the one and only home for imported content.
    ///
    /// Everything the user imports is copied in here, flat, so a .cue and the .bin tracks it
    /// names are always siblings and the cue's relative FILE references resolve. It is inside
    /// the sandbox, so nothing read from it needs a security scope.
    ///
    /// `UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace` are already set in this
    /// app (see project.yml info.properties, which is what actually reaches the built app), so
    /// this directory is visible as "Continuum" in the Files app and over Finder file sharing.
    /// Anything a user drags in there by hand shows up in the Library scan with no extra code:
    /// a free fallback for the picker, and a way to fix up a half-imported game.
    private func documentsDirectory() -> URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    }

    /// Reports whether one of a core's BIOS files is present in the system dir.
    ///
    /// Returns an empty string for a core that declares none, which the HUD then hides: a NES
    /// or SNES cart has nothing to say here, and a permanently empty BIOS line would only
    /// train the eye to ignore the one core that does.
    ///
    /// PCSX ReARMed, the one core that lists any, does not require one: with no BIOS it falls
    /// back to HLE (its `pcsx_rearmed_bios` option / `Config.HLE`) and still boots, at reduced
    /// accuracy. A missing BIOS is therefore a compatibility note, not a hard failure, so it
    /// belongs on the HUD rather than in an error path.
    /// The name of a firmware file this game cannot start without, when it is absent.
    ///
    /// Per SYSTEM rather than per core, which is why it does not use `CoreSpec.biosNames`: fceumm
    /// runs both the NES and the Disk System, the NES needs nothing, and declaring the Disk System's
    /// BIOS against the core would put a "missing firmware" line on every NES game for no reason.
    ///
    /// Nil for every other system, and that is a fact about what we ship rather than an omission.
    /// The PlayStation boots on pcsx_rearmed's HLE, which reimplements the BIOS in code, and the DS
    /// boots on melonDS's FreeBIOS, which is a clean-room replacement. Both are open source, so
    /// both ship. The Disk System has no equivalent.
    private func missingRequiredBios(for entry: LibraryEntry) -> String? {
        guard CoreCatalog.system(forExtension: entry.ext) == .fds else { return nil }
        let name = "disksys.rom"
        guard let dir = systemDirectory() else { return name }
        return FileManager.default.fileExists(atPath: dir.appendingPathComponent(name).path)
            ? nil
            : name
    }

    private func biosStatus(for spec: CoreSpec, in systemDir: URL?) -> String {
        guard !spec.biosNames.isEmpty else { return "" }
        // Every branch names the core. The line is cleared when a core with no BIOS list loads
        // and repopulated when one with a list does, so a line that appears and disappears has
        // to say whose it is or it reads as noise.
        guard let dir = systemDir else {
            return "BIOS (\(spec.coreId)): no system dir, HLE only"
        }
        let present = spec.biosNames.first { name in
            FileManager.default.fileExists(atPath: dir.appendingPathComponent(name).path)
        }
        if let present {
            return "BIOS (\(spec.coreId)): \(present)"
        }
        return "BIOS (\(spec.coreId)): none, HLE fallback"
    }

    /// The engine core states in which a session can actually be started.
    ///
    /// `"loaded"` is resident and idle. `"bound"` is resident and already checked out by a
    /// session, which is usable too: `engine.launch` stops the previous session as its first
    /// act, and that hands the core back to the registry. Reloading a bound core would tear
    /// down a session the engine is about to unwind properly by itself.
    private static let usableCoreStates = ["loaded", "bound"]

    /// Builds the engine declaration for one core spec.
    ///
    /// One place, so the up-front declaration and the re-declaration inside
    /// `ensureCoreLoaded(coreId:)` cannot fill a field in differently. `modulePath` is passed in
    /// rather than derived here because it is the caller that has already resolved and checked
    /// the bundle path.
    private func declaration(for spec: CoreSpec, modulePath: String) -> CoreDeclaration {
        CoreDeclaration(
            id: spec.coreId,
            displayName: spec.displayName,
            systems: spec.systems,
            modulePath: modulePath,
            baseWidth: spec.width,
            baseHeight: spec.height,
            maxWidth: spec.maxWidth,
            maxHeight: spec.maxHeight,
            aspectRatio: spec.aspectRatio,
            targetFps: spec.fps,
            audioSampleRate: spec.sampleRate,
            pixelFormat: spec.pixelFormat,
            priority: spec.priority
        )
    }

    /// Declares all five cores, and loads none of them.
    ///
    /// DECLARING IS NOT LOADING, WHICH IS WHY DOING ALL FIVE UP FRONT IS FREE. `declare_core`
    /// stores a descriptor in the registry and touches no filesystem: it does not dlopen, does
    /// not read the dylib, and does not allocate a core. Only the core a tapped game needs is
    /// ever loaded, in `ensureCoreLoaded(coreId:)`, which is what keeps this build honest about
    /// dynamic loading. Five resident cores would be five emulators' worth of memory for four
    /// systems nobody asked to play.
    ///
    /// The existence check is the other half of the point. A core whose dylib did not make it
    /// into Frameworks/ is named here, on the HUD, before the user taps anything, because from
    /// the Library a missing dylib and a broken core look exactly the same.
    @discardableResult
    func declareAllCores() -> Bool {
        guard let frameworks = Bundle.main.privateFrameworksURL else {
            cores = "cores: no Frameworks directory in the bundle, so none could be declared"
            return false
        }

        var declared: [String] = []
        var missing: [String] = []
        var failed: [String] = []

        for spec in CoreCatalog.all {
            let module = frameworks.appendingPathComponent(spec.library)
            guard FileManager.default.fileExists(atPath: module.path) else {
                // Deliberately NOT declared. An id that was never declared reads back from the
                // engine as nil, so a later tap takes the reload path and reports the missing
                // dylib by name instead of failing inside the loader.
                missing.append("\(spec.coreId) (\(spec.library))")
                continue
            }
            do {
                try engine.declareCore(
                    declaration: declaration(for: spec, modulePath: module.path)
                )
                declared.append(spec.coreId)
            } catch {
                failed.append("\(spec.coreId): \(error.localizedDescription)")
            }
        }

        var line = "cores: \(declared.count) of \(CoreCatalog.all.count) declared"
        if !missing.isEmpty {
            line += " | not in the bundle: \(Self.nameList(missing))"
        }
        if !failed.isEmpty {
            line += " | declare failed: \(Self.nameList(failed))"
        }
        cores = line

        // The BIOS readout is written HERE, at attach, and not only when a core loads. It used
        // to arrive as a side effect of loading PCSX ReARMed at attach; nothing is loaded at
        // attach any more, and a BIOS line that only appears after a disc has been tapped is
        // useless, because knowing the BIOS is absent is what would have changed what the user
        // did. `ensureCoreLoaded` still refreshes it per core, which is what keeps it accurate
        // once something is actually running.
        if let biosCore = CoreCatalog.all.first(where: { !$0.biosNames.isEmpty }) {
            bios = biosStatus(for: biosCore, in: systemDirectory())
        }

        return missing.isEmpty && failed.isEmpty
    }

    /// Makes one core resident, and re-makes it resident whenever the engine has dropped it.
    ///
    /// THE ENGINE IS THE ONLY SOURCE OF TRUTH HERE, AND THAT IS THE FIX. This method used to
    /// cache its success in a Swift `Bool` and early-return on it. That bool went stale, because
    /// the engine unloads the core behind Swift's back: `EmulatorBridge::stop()` returns the core
    /// to the registry and, under the `Drop` retention policy, unloads it, which puts the
    /// registry slot back to `declared`. Every `engine.stop()` therefore invalidated a cached
    /// `true`, whether it came from `stopSession()` or from the canvas teardown that SwiftUI can
    /// run whenever it re-creates the view tree. The next launch then failed with
    /// `CoreUnavailable(reason: "declared but not loaded (state: declared)")` while Swift still
    /// believed the core was loaded, and nothing on screen said otherwise.
    ///
    /// So no bool is kept at all, for any core. The state is read back from the engine on every
    /// call, and the declare + load sequence is re-run whenever that core is not usable, which
    /// makes this path self-healing rather than one-shot. Re-declaring is safe to repeat:
    /// `CoreRegistry::declare` keeps an existing loaded instance and only refreshes metadata, so
    /// it cannot yank a core that is already resident.
    ///
    /// Declare + load only, never a launch: safe to run before any ROM exists, because no core
    /// here touches a game path until `retro_load_game`. Returns `false`, and leaves a specific
    /// reason on the HUD naming the core, if it cannot be made resident.
    @discardableResult
    private func ensureCoreLoaded(coreId: String) -> Bool {
        // An id with no spec is a programming error in the routing table, not a user error, so
        // it gets its own line rather than being reported as a missing dylib.
        guard let spec = CoreCatalog.core(id: coreId) else {
            status = "no core called \(coreId) is shipped in this build"
            return false
        }

        // Ask the engine, never a local flag. `coreState` is Optional and returns nil when the id
        // is unknown to the registry, i.e. nothing has been declared yet, which needs the same
        // reload as an unloaded "declared". Collapsing that nil to "unknown" right here keeps one
        // non-optional string for both the comparison and the HUD, and "unknown" is never a
        // usable state, so an unknown id always takes the reload path below.
        let observed = engine.coreState(coreId: coreId) ?? "unknown"
        if Self.usableCoreStates.contains(observed) {
            // Already usable. No HUD write here on purpose: this is the hot path on every tap,
            // and `launch` names the state it observed in its own breadcrumb, so the state
            // machine stays visible without this stomping on the "opening ..." line.
            return true
        }

        // Name the state that prompted the reload, so the HUD shows the transition and not just
        // its outcome. "failed" gets its own line because a core that loaded and then broke is a
        // different condition from one that was never loaded.
        if observed == "failed" {
            status = "core state is failed; retrying load of \(coreId)..."
        } else {
            status = "core state was \(observed); loading \(coreId)..."
        }

        guard let core = Bundle.main.privateFrameworksURL?
            .appendingPathComponent(spec.library) else {
            status = "no Frameworks directory in the bundle; \(coreId) cannot be loaded"
            return false
        }
        guard FileManager.default.fileExists(atPath: core.path) else {
            status = "\(spec.library) is missing from the bundle, so \(coreId) cannot load"
            return false
        }

        let systemDir = systemDirectory()
        bios = biosStatus(for: spec, in: systemDir)

        do {
            // Declared before loaded, always: `loadNativeCore` refuses an undeclared id
            // rather than inventing geometry for it.
            try engine.declareCore(declaration: declaration(for: spec, modulePath: core.path))
            try engine.loadNativeCore(
                coreId: coreId,
                libraryPath: core.path,
                systemDir: systemDir?.path,
                saveDir: systemDir?.path
            )
        } catch {
            // The HUD is the only diagnostic on a sideloaded build, so the error text lands
            // there rather than throwing into a blank screen. The core id goes with it: five
            // cores means "it failed" is no longer enough to know what failed.
            status = "\(coreId) failed to load: \(error)"
            return false
        }

        // Confirm against the engine rather than concluding success from "the two calls above
        // did not throw". Believing a local success signal over the registry is exactly the
        // mistake the cached bool made, so the result is verified where it actually lives.
        let reloaded = engine.coreState(coreId: coreId) ?? "unknown"
        guard Self.usableCoreStates.contains(reloaded) else {
            status = "\(coreId) load did not take: state is \(reloaded)"
            return false
        }
        return true
    }

    // MARK: The Library

    /// Rescans Documents and republishes the launchable games it holds.
    ///
    /// Writes to `libraryStatus`, never to `status`, so it can be called straight after an
    /// import without erasing that import's summary. Every outcome, including both failures,
    /// gets its own distinct line: no Documents directory, an unreadable directory (with the
    /// real error), an empty directory, a directory with files but nothing launchable, and a
    /// populated Library. There is no silent branch.
    func refreshLibrary() {
        guard let documents = documentsDirectory() else {
            library = []
            libraryStatus = "library unavailable: no Documents directory"
            return
        }

        let contents: [URL]
        do {
            contents = try FileManager.default.contentsOfDirectory(
                at: documents,
                // The date keys are prefetched alongside the size so a rescan of a library of
                // PlayStation discs is still one directory read rather than one stat per file.
                includingPropertiesForKeys: [.fileSizeKey, .contentModificationDateKey,
                                             .creationDateKey],
                options: [.skipsHiddenFiles]
            )
        } catch {
            // Propagated rather than swallowed by `try?`: the real error text is the only
            // thing that tells an I/O fault apart from an empty Documents directory.
            library = []
            libraryStatus = "library scan failed: \(error.localizedDescription) [\(error)]"
            return
        }

        // Only launch targets are listed. The .bin tracks are still right there on disk and
        // the core still opens them; they are simply not things to tap.
        let entries = contents
            .filter { CoreCatalog.launchableExtensions.contains($0.pathExtension.lowercased()) }
            .map { LibraryEntry(url: $0) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        library = entries

        if entries.isEmpty {
            libraryStatus = contents.isEmpty
                ? "library: empty, no files in Documents yet"
                : "library: nothing launchable among \(contents.count) file(s) in Documents "
                    + "(need one of "
                    + "\(CoreCatalog.extensionList(CoreCatalog.launchableExtensions)))"
        } else {
            libraryStatus = "library: \(entries.count) game(s) of "
                + "\(contents.count) file(s) in Documents"
        }

        // Cover art for EVERY game, not only the ones that scroll into view, and without any game
        // being opened. The sweep waits, coalesces the burst of rescans an import produces, and
        // takes one of the three lookup slots so the cards on screen stay ahead of it. See
        // `ArtworkStore.sweepLibrary`. It writes to the artwork line, never to `status` or to
        // `libraryStatus`, so neither an import summary nor the line above is disturbed.
        artwork.sweepLibrary(entries)
    }

    /// Deletes the tapped-away rows from Documents, then rescans.
    ///
    /// Deliberately shallow: it removes the launch target that was swiped, and when that target
    /// was a cue sheet it says out loud that the .bin tracks the sheet named are still on disk.
    /// Guessing which .bin files belonged to a deleted .cue would mean parsing the cue sheet,
    /// and a wrong guess deletes a track another game needs. A cartridge ROM is one file, so
    /// that note is only written when a .cue was actually among the deletions.
    func deleteEntries(at offsets: IndexSet) {
        var removed: [String] = []
        var failures: [String] = []
        var leftTracksBehind = false

        var statesRemoved = 0

        for index in offsets where index >= 0 && index < library.count {
            let entry = library[index]
            do {
                try FileManager.default.removeItem(atPath: entry.path)
                removed.append(entry.name)
                if entry.ext == "cue" { leftTracksBehind = true }
                // The save states go with the game. NOT for tidiness: a state is keyed by the
                // game's filename, so leaving them behind means the next import of a file with the
                // same name inherits states written by a different dump of that game, and the byte
                // length check is the only thing standing between that and a corrupted machine.
                // Counted so the deletion says what else it took.
                let gameId = SaveStates.gameId(for: entry)
                statesRemoved += saveStates.states(forGameId: gameId).count
                saveStates.deleteAll(forGameId: gameId)
                // The cheats too, and for the plainer reason: they are a list about a game that is
                // no longer here, and a reimport would silently inherit codes for another dump.
                cheats.deleteAll(forGameId: gameId)
            } catch {
                failures.append("\(entry.name): \(error.localizedDescription)")
            }
        }

        var trackNote = leftTracksBehind
            ? "; any .bin tracks it named are still in Documents"
            : ""
        if statesRemoved > 0 {
            trackNote += "; \(statesRemoved) save state(s) went with it"
        }

        if removed.isEmpty && failures.isEmpty {
            // Only reachable if the swipe resolved to nothing at all, which would mean the
            // list and the offsets had drifted apart. Say so rather than looking successful.
            status = "delete matched no library row; the list was rescanned"
        } else if failures.isEmpty {
            status = "deleted \(Self.nameList(removed))\(trackNote)"
        } else if removed.isEmpty {
            status = "delete failed: \(Self.nameList(failures))"
        } else {
            status = "deleted \(Self.nameList(removed)); "
                + "delete failed: \(Self.nameList(failures))"
        }

        refreshLibrary()
    }

    // MARK: Import

    /// Copies everything the picker handed back into Documents, permanently.
    ///
    /// This is the whole import architecture in one method, and three of its decisions are
    /// load-bearing:
    ///
    ///   1. FILENAMES ARE PRESERVED EXACTLY. A .cue sheet names its tracks literally, for
    ///      example `FILE "Crash Bandicoot (Track 1).bin" BINARY`, so renaming a .bin to dodge
    ///      a collision would leave a cue pointing at a file that no longer exists and the
    ///      game would simply not load, with nothing on screen to explain why. So there is no
    ///      rename path here at all. A collision REPLACES the existing file (copyItem throws
    ///      if the destination exists, so the old item is removed first) and is reported as
    ///      replaced.
    ///   2. EVERY FILE LANDS IN THE SAME DIRECTORY, flat in Documents, so a cue and its tracks
    ///      are siblings and the cue's relative references resolve.
    ///   3. EVERY FILE IS COPIED INDEPENDENTLY, in its own do/catch. One unreadable file in a
    ///      selection of ten must not abort the other nine; it reports itself and the batch
    ///      carries on.
    func importFiles(_ urls: [URL]) {
        guard let documents = documentsDirectory() else {
            status = "cannot import: no Documents directory"
            return
        }

        var imported: [String] = []
        var replaced: [String] = []
        var rejected: [String] = []
        var failures: [String] = []

        for url in urls {
            let name = url.lastPathComponent
            let ext = url.pathExtension.lowercased()

            // Acceptance is by extension, here, because the picker is deliberately permissive
            // (see `pickerContentTypes`). A rejected file is named on the HUD together with
            // its extension, so "I picked the wrong thing" never looks like "the import broke".
            guard CoreCatalog.importableExtensions.contains(ext) else {
                let extLabel = ext.isEmpty ? "no extension" : ".\(ext)"
                rejected.append("\(name) [\(extLabel)]")
                status = "skipped \(name): \(extLabel) is not content this build can run "
                    + "(want one of "
                    + "\(CoreCatalog.extensionList(CoreCatalog.importableExtensions)))"
                continue
            }

            // The picker was built with asCopy: true, so these URLs are app-owned copies iOS
            // placed in a temporary directory and no security scope should be involved. The
            // pair below is purely defensive, for the case where a provider hands back a
            // scoped URL anyway. Its result is deliberately NOT treated as a failure: an
            // unscoped URL simply returns false and there is then nothing to release, which
            // is why only a true result is balanced with a stop.
            let scoped = url.startAccessingSecurityScopedResource()
            defer {
                if scoped { url.stopAccessingSecurityScopedResource() }
            }

            // The original filename, untouched. See point 1 above.
            let destination = documents.appendingPathComponent(name)

            // Declared outside the do so the catch can say whether the old copy had already
            // been cleared away before the copy failed. That is the one destructive corner of
            // an overwrite, and it must not be silent.
            var replacedExisting = false
            do {
                if FileManager.default.fileExists(atPath: destination.path) {
                    // copyItem throws on an existing destination, so replace rather than
                    // rename. Re-importing a game is a normal thing to do, and it must land
                    // on the same name the cue sheet expects.
                    try FileManager.default.removeItem(at: destination)
                    replacedExisting = true
                }
                try FileManager.default.copyItem(at: url, to: destination)
                imported.append(name)
                // Counted only once the replacement actually landed, so "replaced" never
                // shares a filename with "failed".
                if replacedExisting { replaced.append(name) }
            } catch {
                let lostNote = replacedExisting
                    ? " (the previous copy in Documents was already removed, so import it again)"
                    : ""
                failures.append("\(name): \(error.localizedDescription)")
                status = "import failed for \(name): "
                    + "\(error.localizedDescription)\(lostNote) [\(error)]"
            }
        }

        // The summary replaces whatever per-file line was last written. Per-file lines are for
        // the case where the app dies mid-import; this is the line the user reads afterwards.
        var summary = "imported \(imported.count) of \(urls.count): "
            + Self.nameList(imported)
        if !replaced.isEmpty {
            summary += " | replaced \(replaced.count)"
        }
        if !rejected.isEmpty {
            summary += " | skipped: \(Self.nameList(rejected))"
        }
        if !failures.isEmpty {
            summary += " | failed: \(Self.nameList(failures))"
        }
        status = summary

        // Refresh last, so a freshly imported .cue is tappable straight away. This writes
        // `libraryStatus`, not `status`, so the summary above survives.
        refreshLibrary()
    }

    /// Joins names for the HUD, capped so one line cannot push the rest of the HUD off screen.
    private static func nameList(_ names: [String]) -> String {
        if names.isEmpty { return "none" }
        if names.count <= reportedNameLimit {
            return names.joined(separator: ", ")
        }
        let shown = names.prefix(reportedNameLimit).joined(separator: ", ")
        return "\(shown) +\(names.count - reportedNameLimit) more"
    }

    // MARK: Launch

    /// Launches a game the user tapped in the Library, on the core its extension routes to.
    ///
    /// The path is inside our own Documents directory, so there is NO security scope here and
    /// nothing to acquire, hold or release. That is the entire point of the import step: the
    /// core opens the file, and for a CD game each adjacent .bin the .cue references, with a
    /// plain fopen on files this app owns, and the OS has no grant to refuse. Every core is
    /// handed empty ROM bytes and the real absolute path, which is what the CD cores need
    /// (`need_fullpath`) and what Genesis Plus GX needs for a different reason: the Rust side
    /// splits the real extension into a `ContentHint`, and that is the difference between a
    /// Master System cart booting as a Master System and as a Mega Drive. So `entry.path` is
    /// exactly what the engine wants, and the Rust launch signature did not change to ship
    /// four more cores.
    ///
    /// `resumingAuto` exists for exactly one caller: loading a specific save state from a game's
    /// detail sheet, which launches the game and then loads that state. Restoring the auto-save
    /// first would serialize a megabyte, load it, and throw it away one line later, and the status
    /// line would claim a resume that the next call immediately overwrote. Every other caller wants
    /// the default, which is why it has one.
    func launch(entry: LibraryEntry, resumingAuto: Bool = true) {
        // Breadcrumb, written before the re-entrancy stop and before ANY guard below can
        // return. A tap that reaches this method therefore always changes the HUD. If the HUD
        // ever stays on the previous line, this method provably was not reached, which narrows
        // the fault to the Library row rather than to anything in here.
        let opening = "opening \(entry.name)..."
        status = opening

        // Routing, from the one table the Library row also reads. Resolved BEFORE the stop
        // below on purpose: an unmapped extension is a tap that should change nothing, so a
        // running game is not torn down to report it.
        guard let spec = CoreCatalog.core(forExtension: entry.ext) else {
            let extLabel = entry.ext.isEmpty ? "no extension" : ".\(entry.ext)"
            status = "no core is mapped to \(extLabel), so \(entry.name) cannot be launched"
            return
        }
        // Checked BEFORE anything is torn down, for the same reason the routing above is: this is a
        // tap that should change nothing but the status line.
        //
        // The Disk System is the only system here that cannot boot without a file we are not
        // allowed to ship. Its BIOS is Nintendo's own code, so there is no legal way to put it in
        // the app and no free reimplementation of it the way melonDS carries FreeBIOS for the DS.
        // Without this check the core simply refuses the content and the HUD says "retro_load_game
        // rejected the content", which names neither the cause nor the cure.
        if let missing = missingRequiredBios(for: entry) {
            // Names the file AND the two steps, because the folder the Files app shows is not the
            // folder the core reads: dropping the file in is necessary but not sufficient, and
            // Settings has the one button that crosses the gap. See `installBiosFromDocuments`.
            status = "\(entry.name) needs \(missing), which is Nintendo's own startup file and "
                + "cannot ship with the app. Put it in the Continuum folder in Files, then use "
                + "Settings, Install a BIOS from the Continuum folder"
            return
        }
        // Name the core in the breadcrumb, so a routing bug is one glance rather than a
        // deduction: a .sms that says genesis_plus_gx is right, and one that says anything
        // else is the bug.
        status = "\(opening) on \(spec.coreId)"

        // Re-entrancy: a fresh tap replaces any running session.
        if running {
            stopSession()
        }

        // ORDER IS LOAD-BEARING: THE STOP MUST COME FIRST, AND `ensureCoreLoaded` MUST FOLLOW IT.
        // `stopSession()` calls `engine.stop()`, which under the `Drop` retention policy unloads
        // the core and puts the registry slot back to `declared`. Verifying the core before the
        // stop would therefore verify a core that the stop then invalidates, and the launch below
        // would fail with `state: declared` having just been told the core was fine. Checking
        // after the stop means the state read is the state `engine.launch` will actually see.
        guard ensureCoreLoaded(coreId: spec.coreId) else {
            // ensureCoreLoaded writes its own specific reason on every failure path. Belt
            // and braces: if the status is somehow still the breadcrumb, say plainly that
            // the core is not resident rather than leaving a line that reads like progress.
            if status == opening || status == "\(opening) on \(spec.coreId)" {
                status = "core not loaded: \(spec.library) could not be made resident"
            }
            return
        }

        // Record the state the engine reports at the moment of launch, so a launch that still
        // fails cannot leave the core state as the one unobservable fact in the sequence. This
        // is the line that would have named "declared" out loud instead of costing device trips.
        status = "\(opening) on \(spec.coreId), state "
            + "\(engine.coreState(coreId: spec.coreId) ?? "unknown")"

        // The row was built from a directory scan that may be a few seconds old, and Documents
        // is user-visible through the Files app, so the file can genuinely be gone. Reporting
        // that here keeps it from arriving as an opaque "retro_load_game rejected the content".
        guard FileManager.default.fileExists(atPath: entry.path) else {
            status = "missing on disk: \(entry.name); it was removed since the last scan"
            refreshLibrary()
            return
        }

        do {
            try engine.launch(
                coreId: spec.coreId,
                contentId: entry.name,
                rom: Data(),
                filename: entry.path
            )
            running = true
            // Set HERE, on the success path only, next to `running`. A tap that failed anywhere
            // above must leave the Library on screen with its reason showing, not open a player
            // over a session that does not exist.
            activeEntry = entry
            activeCoreId = spec.coreId
            paused = false
            // Told AFTER `activeEntry`, because `activeSystem` is derived from it. This is what
            // stops "hide the on-screen pad" unmounting the overlay for a system whose touch
            // screen lives on it; see `PhysicalControllers.runningSystemNeedsOverlay`.
            controllers.noteRunningSystem(activeSystem)
            // AFTER the launch, on the success path only. The session is what decides the real
            // output rate, and `audio.start()` reports that rate to the engine, so the sink has
            // to already exist: `launch` is what builds it. Audio that failed to come up does
            // NOT fail the launch, because a silent game is still a playable game, and the reason
            // is on the HUD either way.
            audio.start()
            // Read once here, right after the rate has been reported, so the HUD has a fallback
            // figure even if `AVAudioSession` gave us nothing usable. Not read per frame: the
            // telemetry strip re-renders on every tick and this takes the engine lock.
            engineOutputRate = engine.outputSampleRate()
            refreshAudioReadout()
            status = "running: \(entry.name) on \(spec.coreId)"

            // AFTER the launch, because a cheat table belongs to a session: the core builds it on
            // `retro_load_game` and it dies with the session, so it has to be pushed again every
            // time. Before the resume below on purpose, so that a state restored into this session
            // is restored into a machine that already has the cheats the user expects. The engine
            // re-pushes the table itself after a reset and after a state load, so this is the only
            // place Swift has to do it.
            cheats.push(for: entry)

            // The resume, LAST, so it can overwrite the "running" line above with what actually
            // happened. It is synchronous: see `SaveStates.load` for why nothing here is awaited.
            // A refusal is never fatal, it writes its reason and the game carries on from the
            // beginning, which is the only sensible outcome when the alternative is a state that
            // might corrupt the machine.
            if resumingAuto {
                saveStates.resumeIfPossible(entry: entry)
            }
        } catch {
            running = false
            activeEntry = nil
            activeCoreId = ""
            status = "launch failed on \(spec.coreId): \(entry.name): \(error)"
        }
    }

    /// Stops the current session.
    ///
    /// Nothing else to unwind. This used to also release the picked folder's security scope,
    /// which no longer exists anywhere in this file: imported content lives in our own sandbox,
    /// so there is no scope to balance and no half-wired lifecycle left behind.
    func stopSession() {
        // Audio down FIRST, before `engine.stop()` frees the sink. Ordering matters for the same
        // reason the teardown order inside the bridge does: stopping the engine first would leave
        // a render block being called against a ring whose contents belong to a session that no
        // longer exists. Unconditional, and outside the `running` guard, so a half-started launch
        // cannot leave a live audio graph behind it.
        audio.stop()
        // Both held controls released before the engine stops, for the same reason the pad state
        // is cleared below: a finger still down on fast-forward or rewind when a session ends
        // would leave the engine in that mode with no button on screen to leave it, and the next
        // game would start fast-forwarding or winding backwards on its first frame.
        emulation.releaseHeldControls()
        // THE AUTO-SAVE, AND IT HAS TO BE HERE RATHER THAN ANYWHERE LATER IN THIS FUNCTION.
        // `engine.stop()` below tears the session down and, under the Drop retention policy,
        // unloads the core: after that line there is nothing left to serialize and
        // `retro_serialize` has no session to answer for. The store checks `running` itself and
        // does nothing when there is no session, so a half-started launch that reaches here writes
        // nothing. It is silent on success on purpose, because the line a user is reading when they
        // leave a game is the one `leavePlayer` is about to write.
        saveStates.writeAutoSave(reason: "the game was left")
        if running {
            engine.stop()
            running = false
        }
        // Cleared whether or not a session was running, so a half-started launch cannot leave the
        // player screen up over nothing. The pad state goes with it: a finger still down when a
        // session ends must not be pushed into the next one.
        activeEntry = nil
        activeCoreId = ""
        paused = false
        pictureArea = nil
        // After `activeEntry` is cleared, so a DS session ending releases the hold it had on the
        // overlay and the setting starts working again for the next game.
        controllers.noteRunningSystem(nil)
        padInput.view?.releaseAll()
        refreshAudioReadout()
    }

    // MARK: The player session

    /// Which pad the player screen should draw, from the running game's extension.
    ///
    /// Resolved through the SAME routing table the core was resolved through, so the controls on
    /// screen and the core behind them cannot disagree about what console this is.
    var activeSystem: GameSystem? {
        guard let ext = activeEntry?.ext else { return nil }
        return CoreCatalog.system(forExtension: ext)
    }

    /// The display aspect ratio of the running game, or nil when nothing is running.
    ///
    /// Read from the SAME `CoreSpec` the core was declared to the engine with, so the shape the
    /// picture is given and the shape the engine was told about come from one number rather than
    /// two. Nil while the Library is up, which is what keeps the canvas full bleed behind it
    /// exactly as it always was. See `PictureFit` for what this is used for.
    var activePictureAspect: CGFloat? {
        guard !activeCoreId.isEmpty,
              let spec = CoreCatalog.core(id: activeCoreId),
              spec.aspectRatio > 0 else { return nil }
        return CGFloat(spec.aspectRatio)
    }

    /// Leaves the running game and goes back to the Library.
    ///
    /// Delegates to `stopSession()` rather than calling `engine.stop()` itself, so there is still
    /// exactly one place that stops a session and the Drop retention policy's unload happens on the
    /// path the launch sequence already depends on.
    func leavePlayer() {
        let leaving = activeEntry?.name
        stopSession()
        // A distinct line either way. "Stopped nothing" is a real condition worth seeing: it means
        // the back control was reached with no session, which points at the launch path rather than
        // at this one.
        if let leaving {
            status = "stopped \(leaving); back in the library"
        } else {
            status = "left the player with no session running"
        }
        refreshLibrary()
    }

    func togglePause() {
        guard running else {
            status = "pause ignored: no game is running"
            return
        }
        if paused {
            // The same clock MetalCanvas hands the engine on didBecomeActive. The pacer takes a
            // timestamp and never reads a clock of its own, so resuming from a different time base
            // would make it think every frame was late.
            engine.resume(nowMillis: CACurrentMediaTime() * 1000.0)
            paused = false
            status = "resumed \(activeEntry?.name ?? "the session")"
        } else {
            engine.pause()
            paused = true
            status = "paused \(activeEntry?.name ?? "the session")"
        }
    }

    func resetGame() {
        guard running else {
            status = "reset ignored: no game is running"
            return
        }
        do {
            try engine.reset()
            status = "reset \(activeEntry?.name ?? "the session")"
        } catch {
            status = "reset failed on \(activeEntry?.name ?? "the session"): \(error)"
        }
    }

    /// Saves the running game into its next free numbered slot.
    ///
    /// A one-line delegation, and the shape is the point. This used to be the whole save-state
    /// feature: it wrote ONE file per game, `<filename>.state`, into Documents, and nothing in the
    /// app ever read it back. Documents looked like the right home because `UIFileSharingEnabled`
    /// already exposes it, so a state could be copied off the device, but a slot system cannot live
    /// somewhere its files can be renamed or deleted underneath the index that describes them. See
    /// the header of SaveStates.swift for that argument and for the compatibility gate, which is the
    /// part that could not exist at all while a state carried no metadata.
    func saveStateToSlot() {
        saveStates.saveToNewSlot()
    }

    /// Launches a game and immediately loads one of its states.
    ///
    /// The detail sheet's "play from this state", and the reason `launch` takes `resumingAuto`. A
    /// launch is synchronous all the way to a running session, so by the time it returns there is a
    /// core to load into; if it failed, `running` is false and the store refuses with its own
    /// reason rather than this method inventing one.
    func launchAndLoad(entry: LibraryEntry, record: SaveStateRecord) {
        launch(entry: entry, resumingAuto: false)
        guard running else { return }
        saveStates.load(record)
    }

    /// Takes a line from the control surface.
    ///
    /// Only reachable today when two controls were laid out on top of each other, which is the one
    /// failure the layout arithmetic cannot rule out by itself and exactly the bug
    /// docs/mobile-player.png captured. It lands in its own field so it cannot be overwritten by
    /// the next status line.
    func noteControlLayout(_ line: String) {
        controlNote = line
        // Surfaced on the always-visible line too, because a layout fault is not something to
        // find only after opening the diagnostics panel.
        status = line
    }

    /// Records the area the picture may use, as reported by the control surface.
    ///
    /// Hopped to the next run loop turn because it arrives from a UIKit layout pass, and writing
    /// published state in the middle of one is how a SwiftUI update loop starts. The control
    /// surface already suppresses unchanged values, so this does not fire every frame.
    func updatePictureArea(_ rect: CGRect) {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            // A rect from an overlay that is on its way off the screen is not the truth about the
            // picture, and because this write is deferred by a run loop turn it would land AFTER
            // `PlayerScreen` cleared the area for exactly that reason. The result would be a
            // letterbox held open for controls nobody can see, until the next time something else
            // changed. The unmount path cannot be relied on to stay silent: a view being removed can
            // still be laid out on the way out.
            guard !self.controllers.hidesOnScreenPadNow else { return }
            guard self.pictureArea != rect else { return }
            self.pictureArea = rect
        }
    }

    // MARK: The read-outs

    /// The banner line, unchanged in substance from the one the HUD carried.
    /// Counted from the catalogue rather than written out, because this line is the first thing
    /// read in every screenshot and a hardcoded number goes stale the moment a core is added. It
    /// said "five" while six were shipping.
    var buildLine: String {
        "\(Self.versionLabel) - \(CoreCatalog.all.count) libretro cores (software)"
    }

    /// Version and build number of the copy that is actually running, e.g. `0.8.0 (76)`.
    ///
    /// THE POINT OF THIS IS TO TELL A NEW INSTALL FROM A FAILED ONE AT A GLANCE. CFBundleVersion
    /// was a hardcoded "1" for this project's whole life, so every build was 0.8.0 (1) under one
    /// bundle id, and an installer that compares those numbers treats a new .ipa as the copy
    /// already present and skips it. It now carries the CI run number, which means this line
    /// changes when an install genuinely replaced the app and stays put when it did not. An evening
    /// went into a core that was in the bundle the whole time and simply was not installed.
    static var versionLabel: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "Continuum \(short) (\(build))"
    }

    var frameLine: String {
        let fps = String(format: "%.0f", displayFps)
        return "\(frameCount) frames - \(fps) fps - \(dropped) dropped"
    }

    /// Audio, end to end, with nothing assumed.
    ///
    /// This line used to say "no output path yet" and derive its milliseconds from a hard-coded
    /// 48000, because nothing drained the engine's ring and nothing could tell the engine what
    /// rate the device ran at. Both are now false, so every number here is measured:
    ///
    /// - the rate is the one `AVAudioSession` reported after the session was activated, passed
    ///   into the engine so the resampling happens in Rust;
    /// - the latency is BOTH rings added together, because the engine's ring is drained into the
    ///   device's ring every tick and what a player hears is the sum of the two;
    /// - the underruns are counted separately at each end, since an engine underrun means the
    ///   core did not produce in time while a device underrun means the tick did not push in
    ///   time, and those have completely different causes;
    /// - "silent" is reported when the graph is up but the render block has never run, which is
    ///   the one failure that would otherwise look exactly like working audio.
    var audioLine: String {
        guard audioRunning else {
            return "audio: not running - \(audioStatus)"
        }
        let rate = audioDeviceRate > 0 ? audioDeviceRate : Double(engineOutputRate)
        let frames = Int(audioQueued) + audioDeviceBuffered
        let ms = rate > 0 ? Double(frames) / rate * 1000.0 : 0
        let played = audioRenderedFrames > 0
            ? "playing"
            : "SILENT (the render block has not run)"
        var line = "audio: \(played), \(frames) frames buffered ("
            + String(format: "%.0f", ms) + " ms) at \(Int(rate)) Hz, "
            + "\(audioUnderruns) engine + \(audioRenderUnderruns) device underrun(s)"
        // Shown only when it says something. A core at the device's own rate is a passthrough and
        // does not need announcing; a core at 32040 Hz being stretched to 48000 does, because that
        // is the path any pitch or quality complaint would start at.
        if audioSourceRate > 0, audioSourceRate != engineOutputRate {
            line += ", resampling \(audioSourceRate) to \(engineOutputRate) Hz"
        }
        if audioOverruns > 0 {
            line += ", \(audioOverruns) engine overrun(s)"
        }
        // The early warning that precedes an overrun. In steady state the tick empties the engine
        // ring every frame, so a half-full one means the pump is not keeping up and audio is about
        // to start being dropped: worth seeing BEFORE the overrun counter moves.
        if audioCapacityFrames > 0, audioQueued * 2 > audioCapacityFrames {
            line += ", engine ring \(audioQueued)/\(audioCapacityFrames) and filling"
        }
        if audioDroppedFrames > 0 {
            line += ", \(audioDroppedFrames) frame(s) dropped at the device"
        }
        return line + " - \(audioStatus)"
    }

    /// Copies the audio object's state into published properties.
    ///
    /// Called from the telemetry callback, which runs on the main thread inside the display link,
    /// and from the launch and stop paths so the line is right the moment either happens. Reading
    /// the ring's counters is a handful of word loads with no lock and no allocation; the strings
    /// are only reassigned when they differ, so a running game does not rebuild them sixty times
    /// a second.
    func refreshAudioReadout() {
        if audioRunning != audio.isRunning { audioRunning = audio.isRunning }
        if audioStatus != audio.status { audioStatus = audio.status }
        if audioDeviceRate != audio.sampleRate { audioDeviceRate = audio.sampleRate }
        audioDeviceBuffered = audio.ring.bufferedFrames
        audioRenderUnderruns = audio.ring.underruns
        audioRenderedFrames = audio.ring.renderedFrames
        audioDroppedFrames = audio.ring.droppedFrames

        // The engine's half, on a one second timer. See `audioOverruns` for why this is not read
        // every frame. Counted down rather than derived from the frame number, so it keeps working
        // at 120 Hz and across a session that restarted the counter.
        audioStatsCountdown -= 1
        if audioStatsCountdown <= 0 {
            audioStatsCountdown = 60
            let stats = engine.audioStats()
            audioOverruns = stats.overruns
            audioSourceRate = stats.sourceRate
            audioCapacityFrames = stats.capacityFrames
            // Re-read rather than trusted from launch time: a route change reconfigures the sink
            // behind Swift's back, and a stale rate here would silently make the latency figure
            // wrong again.
            engineOutputRate = stats.outputRate
        }
    }

    /// What is driving input right now: the on-screen pad, and every physical controller.
    ///
    /// The two are reported separately because they are separate ENGINE LAYERS, and telling them
    /// apart is the whole diagnosis when input misbehaves. The overlay writes the `.touch` layer
    /// and controllers write `.gamepad`; the engine merges them when the core reads input. A
    /// controller that shows here and still does nothing in the game is a mapping or a core
    /// problem, while a controller that does not show here never reached the app at all, and
    /// without both halves on one line those two look identical.
    ///
    /// `physicalPads` is deliberately NOT what the controller half is built from. See its own note.
    var inputLine: String {
        var pads = controllers.diagnosticLine
        // Appended only when it says something, the way `audioLine` treats its resampling note. The
        // engine's own pad table cannot be written from Swift today, so a zero here is the expected
        // value and printing it beside a real controller count would read as a contradiction.
        if physicalPads > 0 {
            pads += ", engine pad table \(physicalPads)"
        }
        guard let system = activeSystem else {
            return "input: no pad on screen (no game running), \(pads)"
        }
        guard !controllers.hidesOnScreenPadNow else {
            return "input: on-screen pad hidden by the controller setting, \(pads)"
        }
        return "input: \(system.badge) pad on port 0, \(system.controlCount) button(s) plus the "
            + "D-pad, \(pads)"
    }

    /// The on-screen pad's arrangement in one line, for the Settings row that opens the editor.
    ///
    /// Says "default" when it is the shipped one rather than printing the same six numbers a fresh
    /// install would, because the useful question that row answers is whether anything has been
    /// changed. The numbers are still spelled out once it has been, since they are what persists and
    /// they are the only way to tell two similar arrangements apart.
    var touchLayoutLine: String {
        let live = touchLayout.sanitised
        let size = "\(Int((live.scale * 100).rounded()))%"
        let opacity = "\(Int((live.opacity * 100).rounded()))%"
        guard !live.isStandard else {
            return "default: size \(size), opacity \(opacity)"
        }
        let positions = String(format: "d-pad %.2f, %.2f  buttons %.2f, %.2f",
                               live.dpadX, live.dpadY, live.faceX, live.faceY)
        return "size \(size), opacity \(opacity), \(positions)"
    }

    /// The thin strip on the player screen.
    ///
    /// The audio figure is the SUM of both rings over the device's real rate, same as
    /// `audioLine`. When the graph is not up it says so in a word rather than printing a
    /// millisecond figure for a buffer nothing is reading, which is what the old strip did.
    var telemetryLine: String {
        let fps = String(format: "%.0f", displayFps)
        let core = activeCoreId.isEmpty ? "no core" : activeCoreId
        let audioPart: String
        if !audioRunning {
            audioPart = "no audio"
        } else if audioRenderedFrames == 0 {
            audioPart = "audio silent"
        } else {
            let rate = audioDeviceRate > 0 ? audioDeviceRate : Double(engineOutputRate)
            let frames = Int(audioQueued) + audioDeviceBuffered
            let ms = rate > 0 ? Double(frames) / rate * 1000.0 : 0
            audioPart = String(format: "%.0f", ms) + " ms audio"
        }
        return "\(fps) fps - \(frameCount) frames - \(dropped) dropped - "
            + "\(audioPart) - \(core)"
    }

    /// What the engine says it is resampling to, read once when audio comes up.
    ///
    /// A fallback for the read-outs above, used only before the session has reported a rate.
    /// Read once rather than per frame for the same mutex reason as `activeCoreId`: the strip
    /// re-renders on every tick, and `outputSampleRate()` takes the engine lock.
    @Published var engineOutputRate: UInt32 = 0

    /// The core id of the running session, cached at launch.
    ///
    /// Cached rather than read from `engine.currentCoreId()` where it is displayed, and that is not
    /// premature: the telemetry strip re-renders on every tick, so a read there would take the
    /// engine's mutex sixty times a second, on the same thread that holds it for the whole of each
    /// tick. The id cannot change without going through `launch` or `stopSession`, both of which
    /// set this.
    @Published var activeCoreId: String = ""

    /// How many pads the ENGINE'S OWN connection table has registered, read once when the surface
    /// attaches.
    ///
    /// Expected to be 0 forever, still, and that is correct rather than broken. `connect_pad` is
    /// not exported through UniFFI, so nothing in Swift can register a pad in that table, and
    /// `apply_gamepad_from` writes an input layer without consulting it. Real controllers are
    /// therefore tracked on this side, in `PhysicalControllers`, and `inputLine` reports that
    /// instead. This figure is kept because the day `connect_pad` is exported, a non-zero value
    /// here is how you will know it worked. Read once rather than per frame for the same mutex
    /// reason as `activeCoreId`.
    @Published var physicalPads: UInt32 = 0

    // MARK: Presenting the picker

    /// Presents the import picker directly from the live UIKit hierarchy.
    ///
    /// This replaces SwiftUI's `.fileImporter`, which was proven on device not to call its
    /// completion closure at all: the sheet dismissed and the HUD, whose very first statement
    /// in that closure was an unconditional breadcrumb, did not change. SwiftUI's presentation
    /// machinery is therefore the thing that failed, so the picker is presented straight from
    /// the window's topmost view controller instead of being routed back through a SwiftUI
    /// sheet.
    func presentImportPicker() {
        // Breadcrumb written BEFORE anything else, so the button press itself is observable
        // even if resolving a presenter fails. If the HUD never reaches this line, the button
        // action is not running at all.
        status = "presenting import picker..."

        guard let presenter = Self.topmostViewController() else {
            status = "cannot present picker: no root view controller"
            return
        }

        // asCopy: true is the architecture. iOS hands back app-owned copies in a temporary
        // directory, which is precisely what an import flow wants, and it removes the
        // security-scope dependency that kept failing when the app tried to read the user's
        // original files in place. `importFiles` then moves those copies to their permanent
        // home in Documents.
        let picker = UIDocumentPickerViewController(
            forOpeningContentTypes: Self.pickerContentTypes,
            asCopy: true
        )
        // The weak delegate is pointed at the property that owns it, never at a local. See
        // `importPickerDelegate` for why that distinction is a bug rather than a detail.
        picker.delegate = importPickerDelegate
        // Multi-select is the point: a CD game is a .cue plus its .bin tracks, and they have
        // to arrive together or the cue's references dangle.
        picker.allowsMultipleSelection = true
        // Show extensions so the user can confirm by eye that they have the .cue AND the .bin.
        picker.shouldShowFileExtensions = true

        // Watches for a dismissal that yields no pick at all: a swipe-away, or something else
        // tearing the sheet down. Without this, "the user or the system closed the sheet" and
        // "the pick callback never fired" are the same blank HUD, which is the one ambiguity
        // still open. `presentationController` is created on demand from
        // `modalPresentationStyle` for a controller that has not been presented yet, so it is
        // expected to be non-nil; the pre-present line below reports whether the watch was
        // actually wired rather than leaving a silently unhooked callback.
        picker.presentationController?.delegate = importPickerDelegate
        let dismissalWatch = picker.presentationController == nil
            ? "no dismissal watch"
            : "dismissal watch on"

        // Arm the delegate for this picker: clears the flag that suppresses the dismissal
        // notice, so a second pick attempt is tracked as carefully as the first.
        importPickerDelegate.prepareForNewPicker()

        // Hold the picker so an early release cannot be the invisible cause of a silent HUD.
        activePicker = picker

        // Naming the presenter is the point of this line. If the picker is being presented
        // from something unexpected or already detached from the window, this is the smoking
        // gun, and it is only readable BEFORE the call in case the call itself never returns.
        let presenterName = String(describing: type(of: presenter))
        status = "presenting import picker from \(presenterName) (\(dismissalWatch))..."

        // The completion handler is the fix to a LYING diagnostic. `present` is asynchronous,
        // so an next-line "picker presented" claim is written whether or not the presentation
        // ever finished, which made "stuck waiting for the delegate" mean two different things:
        // a silent delegate, or a picker that never truly came up. Only the completion handler
        // runs once UIKit has actually finished presenting, so only a HUD line written in here
        // can honestly claim the sheet is up.
        presenter.present(picker, animated: true) { [weak self] in
            // UIKit runs this on the main thread, but the closure carries no isolation the
            // compiler can see under SWIFT_VERSION 5.0, so the hop is explicit. `EngineHost`
            // is global-actor isolated and therefore Sendable, so a weak capture of it is
            // safe to carry across.
            Task { @MainActor in
                self?.status = "picker is up; select the .cue and every .bin track"
            }
        }
    }

    /// Drops the strong reference to the presented picker once it has reported back.
    ///
    /// Called from every delegate callback. Not private: `ImportPickerDelegate` is a separate
    /// type by necessity (a UIKit delegate must be an `NSObject`) and is the only caller.
    func releaseActivePicker() {
        activePicker = nil
    }

    /// Resolves the view controller to present from: the topmost one in the active window.
    ///
    /// Walks `connectedScenes` for the foreground window scene, takes its key window, then
    /// follows `presentedViewController` to the top so the picker is presented from whatever
    /// is actually on screen rather than from a controller that is already covered, which
    /// UIKit would refuse. Returns nil only when there is genuinely no window to present
    /// from; the caller turns that into its own HUD line rather than failing silently.
    ///
    /// Internal rather than private only so the artwork picker can reuse it. Writing a second
    /// presenter resolver would be two copies of the one piece of UIKit plumbing in this app that
    /// cannot be tested anywhere but a device, and the second copy is always the one that is wrong.
    static func topmostViewController() -> UIViewController? {
        let windowScenes = UIApplication.shared.connectedScenes.compactMap {
            $0 as? UIWindowScene
        }
        let scene = windowScenes.first { $0.activationState == .foregroundActive }
            ?? windowScenes.first
        guard let scene else { return nil }

        let window = scene.keyWindow
            ?? scene.windows.first { $0.isKeyWindow }
            ?? scene.windows.first
        guard var top = window?.rootViewController else { return nil }

        while let presented = top.presentedViewController {
            top = presented
        }
        return top
    }

    /// Handles what the import picker handed back, on the main actor.
    ///
    /// Split out of the delegate callback on purpose. `UIDocumentPickerDelegate`'s methods
    /// carry no actor isolation the compiler can see, so anything they do to this `@MainActor`
    /// object has to cross an actor hop first; mutating a `@Published` property from off the
    /// main actor may never reach the UI, which is one way a pick can dismiss the sheet and
    /// change nothing on screen. The delegate only flattens what it received and hops here.
    func handlePicked(_ outcome: PickerOutcome) {
        // Unconditional breadcrumb: the first statement of the first main-actor code that runs
        // after a pick, ahead of every guard and early return. It proves the handler fired and
        // shows the raw shape of what came back. A failure line below replaces it immediately;
        // that is intended, because a specific error beats a breadcrumb.
        status = "picked \(outcome.urls.count) file(s): "
            + "\(outcome.urls.first?.lastPathComponent ?? "none")"

        if let failure = outcome.failureText {
            status = failure
            return
        }
        guard !outcome.urls.isEmpty else {
            status = "picker returned no files"
            return
        }
        importFiles(outcome.urls)
    }

    func surfaceAttached(_ result: Result<String, Error>) {
        switch result {
        case .success(let summary):
            gpu = summary
            // Read once, here, so the diagnostics panel can show it without the telemetry strip
            // taking the engine's mutex on every frame. Expected to be 0; see `physicalPads`.
            physicalPads = engine.connectedPads()
            // The engine's default output rate, before any session has said otherwise. Shown so
            // the audio line has a rate to print while the Library is up, and so a device whose
            // session later reports something else makes the change visible rather than silent.
            engineOutputRate = engine.outputSampleRate()
            // Declare all five cores and LOAD NONE OF THEM. Which core is needed is not known
            // until a game is tapped, and nothing here can be auto-booted anyway: these cores
            // need real content and hard-reject empty content. Declaring costs no dlopen, so
            // the Library tap goes straight to loading exactly one core.
            let allCoresPresent = declareAllCores()
            refreshLibrary()
            if !allCoresPresent {
                // The `cores` line already names what is wrong. Say out loud that it is worth
                // reading rather than leaving a cheerful ready line above a broken bundle.
                status = "surface ready, but not every core is in the bundle - read the cores line"
            } else {
                status = library.isEmpty
                    ? "surface ready - tap Import Games to add a game"
                    : "surface ready - tap a game in the Library"
            }
        case .failure(let error):
            status = "attach failed: \(error)"
        }
    }
}

// MARK: - The picker's delegate

/// The `UIDocumentPickerViewController` delegate.
///
/// A separate `NSObject` subclass rather than a conformance on `EngineHost`, because a UIKit
/// delegate must be an `NSObject` while `EngineHost` is a plain `@MainActor` `ObservableObject`.
///
/// LIFETIME, which is the whole point of this type: the picker holds its `delegate` WEAKLY, so
/// this object only survives long enough to be called back because `EngineHost` owns it through
/// its `importPickerDelegate` property, and `EngineHost` is itself owned by a `@StateObject` for
/// the app's lifetime. The reference back to the host is `unowned` so the two do not form a
/// retain cycle; it is safe because this object cannot outlive the host that owns it.
/// Both pick callbacks are implemented, and both name themselves on the HUD. The modern iOS 14+
/// array form is the one that should fire, and it is also the one that carries a multi-file
/// selection; the deprecated single-URL form is kept alongside it, never instead of it, purely
/// as a diagnostic. If the array selector genuinely is not being delivered on this OS build,
/// the single-URL one will fire and the HUD will say which, so the question "is the selector
/// wrong, or is the callback never sent at all" is answered by reading the screen rather than
/// by another round of guessing. Every callback is `@objc` explicitly: these are optional
/// protocol requirements, dispatched by selector from Objective-C, and none of them should
/// depend on the compiler inferring an `@objc` entry point.
///
/// `UIAdaptivePresentationControllerDelegate` is here for the same reason: it separates "the
/// sheet was closed without a pick" from "the pick callback stayed silent".
final class ImportPickerDelegate: NSObject, UIDocumentPickerDelegate,
                                  UIAdaptivePresentationControllerDelegate {
    private unowned let host: EngineHost

    /// Set as soon as any pick or cancel callback fires, and read only by the dismissal notice.
    ///
    /// The picker dismisses itself after a pick and after a cancel, so the dismissal callback can
    /// arrive on a perfectly successful run. Without this flag it would overwrite the import
    /// summary with "dismissed without a pick" and manufacture exactly the sort of false report
    /// this file exists to eliminate. Written synchronously on the thread UIKit calls back on,
    /// never inside the actor hop, so it is always set before any later callback can read it.
    private var outcomeDelivered = false

    init(host: EngineHost) {
        self.host = host
        super.init()
    }

    /// Re-arms dismissal tracking for a freshly built picker. Called by `presentImportPicker`.
    func prepareForNewPicker() {
        outcomeDelivered = false
    }

    /// The modern iOS 14+ callback, and the one that is expected to fire. Its first act is a
    /// HUD write, before any processing. This is the callback that carries a multi-file
    /// selection, which is the whole reason `allowsMultipleSelection` is on.
    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentsAt urls: [URL]) {
        // Unconditional breadcrumb, built from nothing but what was handed in, ahead of every
        // branch below. It proves the delegate fired and shows the shape of what came back, so
        // "the callback never ran" and "the callback ran and then something failed" can never
        // again be confused for each other. It names itself multi-url so it is distinguishable
        // at a glance from the single-url fallback.
        let fired = "delegate fired (multi-url): \(urls.count) url(s): "
            + "\(urls.first?.lastPathComponent ?? "none")"
        // An empty pick is its own reported condition, not a silent no-op.
        let outcome = PickerOutcome(
            urls: urls,
            failureText: urls.isEmpty ? "delegate fired with 0 urls: nothing to import" : nil
        )
        deliver(fired, outcome)
    }

    /// The deprecated iOS 8 single-URL callback, kept ALONGSIDE the array form above.
    ///
    /// This is not the primary path and is not expected to fire. It exists so that the failure
    /// mode "the array selector is not being delivered" cannot hide: if this one runs, the HUD
    /// says so by name, and if neither runs then no pick callback is being sent at all and the
    /// signature was never the problem. It forwards into the SAME outcome and import path with
    /// the single URL wrapped in an array, so there is exactly one downstream behaviour.
    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentAt url: URL) {
        let fired = "delegate fired (single-url): \(url.lastPathComponent)"
        deliver(fired, PickerOutcome(urls: [url], failureText: nil))
    }

    /// Distinguishes "the user backed out" from "the callback never fired".
    ///
    /// Without this line those two look identical on the HUD, which is exactly the ambiguity
    /// that made earlier iterations impossible to diagnose.
    @objc func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        outcomeDelivered = true
        let host = self.host
        Task { @MainActor in
            host.status = "picker cancelled: no files chosen"
            host.releaseActivePicker()
        }
    }

    /// Reports a sheet that went away without producing a pick.
    ///
    /// This is the other half of the "stuck waiting for the delegate" ambiguity: a picker that
    /// was swiped away, or torn down by something else, versus a picker that is still up with a
    /// silent delegate. Those now read differently on the HUD.
    @objc func presentationControllerDidDismiss(
        _ presentationController: UIPresentationController
    ) {
        // A pick or a cancel has already spoken for this picker and the picker dismisses itself
        // afterwards, so staying quiet here is deliberate: the real result must not be replaced
        // by a "no pick" line. Every other branch of this type writes to the HUD.
        if outcomeDelivered {
            return
        }
        let host = self.host
        Task { @MainActor in
            host.status = "picker dismissed without a pick (swipe or programmatic)"
            host.releaseActivePicker()
        }
    }

    /// The one path from a pick to the main actor, shared by both pick callbacks.
    ///
    /// UIKit calls the callbacks on the main thread, but the compiler cannot know that under
    /// SWIFT_VERSION 5.0, so the hop to the `@MainActor` host is explicit. `EngineHost` is
    /// global-actor isolated and therefore Sendable, so it is bound to a local first and the
    /// non-Sendable delegate itself is never captured by the task.
    private func deliver(_ fired: String, _ outcome: PickerOutcome) {
        outcomeDelivered = true
        let host = self.host
        Task { @MainActor in
            host.status = fired
            host.releaseActivePicker()
            host.handlePicked(outcome)
        }
    }
}

// MARK: - The view

/// The app's one root: the drawing surface, plus whichever screen is in front of it.
///
/// THE METAL CANVAS IS MOUNTED ONCE AND NEVER UNMOUNTED, AND THAT IS THE REASON THIS VIEW IS
/// SHAPED LIKE THIS RATHER THAN AS A NavigationStack. `attachMetal` hands the engine this view's
/// `CAMetalLayer` and the engine then owns the surface, its device, its queue and its swapchain
/// configuration. Pushing a player screen that replaced the canvas would tear that layer down and
/// force a second attach against a new one, on the one path in this app that cannot be tested
/// anywhere but a device. So the canvas stays at the bottom of this stack for the app's lifetime
/// and the screens cover it: the Library opaquely, the player around the picture.
struct RootView: View {
    /// Owns the engine host for the app's lifetime, which is also what keeps the picker's
    /// delegate alive long enough to be called back. See `EngineHost.importPickerDelegate`.
    @StateObject private var host = EngineHost()

    var body: some View {
        ZStack(alignment: .topLeading) {
            // Black under everything, so the letterbox the renderer leaves around a 4:3 picture is
            // black rather than whatever the compositor had there.
            Color.black.ignoresSafeArea()

            canvas

            if host.activeEntry == nil {
                // The library shell, opaque over the canvas. See LibraryShell.swift for why it
                // covers the canvas rather than replacing it.
                LibraryShell(host: host, artwork: host.artwork)
            } else {
                PlayerScreen(host: host,
                             emulation: host.emulation,
                             // Observed by the player screen rather than read through the host,
                             // because a pad connecting has to redraw it: it decides whether the
                             // on-screen controls are mounted at all.
                             controllers: host.controllers,
                             // Observed for the same kind of reason: the save button's menu lists
                             // this game's states, so writing one has to change the menu without
                             // anything else on the player redrawing.
                             saveStates: host.saveStates,
                             system: host.activeSystem)
            }
        }
        .background(.black)
    }

    /// The one canvas, sized to the area the controls left free.
    ///
    /// `pictureArea` is nil until a player screen has laid its controls out, and while the Library
    /// is up, in which case the canvas takes the whole window exactly as it always did. When it is
    /// set, the canvas is positioned into that rect and `MetalCanvas.layoutSubviews` forwards the
    /// new extent through the `resizeSurface` path it already used for rotation. Nothing about the
    /// attach changes; only the size does.
    private var canvas: some View {
        let padInput = host.padInput
        let controllers = host.controllers
        return GeometryReader { proxy in
            let region = host.pictureArea ?? CGRect(origin: .zero, size: proxy.size)
            // Centred in the free region and shaped like the game, rather than stretched across a
            // region whose shape has nothing to do with the picture. See `PictureFit`.
            let area = host.activePictureAspect
                .map { PictureFit.rect(aspect: $0, in: region) } ?? region
            MetalCanvasView(
                engine: host.engine,
                // Captured as a local reference so this closure touches the box and nothing else,
                // which keeps the display link's read away from the main-actor host entirely.
                gamepadSource: { padInput.currentFrame() },
                // The other input source, on the engine's other layer. Captured as a local for half
                // the reason the line above is: so the closure does not reach through `host` and
                // republish the library shell from inside the render loop. The isolation half does
                // not apply, because unlike the pad box this object IS main-actor isolated, and the
                // display link is the main thread. Empty while nothing is attached, which is what
                // keeps the gamepad layer untouched rather than being told "nothing held" sixty
                // times a second.
                controllerSource: { controllers.poll() },
                // Handed over so the display link can pump it immediately after each step. The
                // object is owned by the host, so a SwiftUI rebuild of this view cannot take the
                // audio graph down with it.
                audio: host.audio,
                onAttach: { host.surfaceAttached($0) },
                onTelemetry: { telemetry in
                    host.frameCount = telemetry.frameCount
                    host.displayFps = telemetry.displayFps
                    host.dropped = telemetry.dropped
                    host.audioQueued = telemetry.audioQueuedFrames
                    host.audioUnderruns = telemetry.audioUnderruns
                    // Reads a handful of words out of the lock-free ring. This is where "the
                    // graph is up but nothing is playing" becomes visible, which on a sideloaded
                    // build is the difference between a diagnosis and a guess.
                    host.refreshAudioReadout()
                }
            )
            .frame(width: max(1, area.width), height: max(1, area.height))
            .position(x: area.midX, y: area.midY)
        }
        .ignoresSafeArea()
    }
}

/// Bridges `MetalCanvas` into SwiftUI, and wires the app lifecycle to it.
struct MetalCanvasView: UIViewRepresentable {
    let engine: ContinuumEngine
    /// Read once per frame inside the display link, immediately before the engine step. See
    /// `MetalCanvas.gamepadSource` for why it has to be pulled per frame rather than pushed on a
    /// touch.
    let gamepadSource: () -> PadFrame
    /// Every attached physical controller, read in the same tick and pushed to the engine's OTHER
    /// input layer. See `MetalCanvas.controllerSource` for why the two must not share one.
    let controllerSource: () -> [ControllerFrame]
    /// The device audio path, pumped from the display link. Owned by the host; see
    /// `MetalCanvas.audio` for why the push has to happen there and not on the audio thread.
    let audio: AudioOutput
    let onAttach: (Result<String, Error>) -> Void
    let onTelemetry: (TickTelemetry) -> Void

    func makeUIView(context: Context) -> MetalCanvas {
        let canvas = MetalCanvas(engine: engine)
        canvas.onAttach = onAttach
        canvas.onTelemetry = onTelemetry
        canvas.gamepadSource = gamepadSource
        canvas.controllerSource = controllerSource
        canvas.audio = audio
        canvas.start()
        context.coordinator.observe(canvas)
        return canvas
    }

    func updateUIView(_ canvas: MetalCanvas, context: Context) {
        canvas.onTelemetry = onTelemetry
        canvas.gamepadSource = gamepadSource
        canvas.controllerSource = controllerSource
        canvas.audio = audio
    }

    static func dismantleUIView(_ canvas: MetalCanvas, coordinator: Coordinator) {
        canvas.stop()
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    /// Lifecycle notifications, forwarded in the order section 10 of the design document
    /// requires: checkpoint, then release graphics, never the other way round.
    final class Coordinator {
        private var canvas: MetalCanvas?
        private var observers: [NSObjectProtocol] = []

        func observe(_ canvas: MetalCanvas) {
            self.canvas = canvas
            let centre = NotificationCenter.default
            observers = [
                centre.addObserver(forName: UIApplication.willResignActiveNotification,
                                   object: nil, queue: .main) { [weak canvas] _ in
                    canvas?.willResignActive()
                },
                centre.addObserver(forName: UIApplication.didEnterBackgroundNotification,
                                   object: nil, queue: .main) { [weak canvas] _ in
                    canvas?.didEnterBackground()
                },
                centre.addObserver(forName: UIApplication.willEnterForegroundNotification,
                                   object: nil, queue: .main) { [weak canvas] _ in
                    canvas?.willEnterForeground()
                },
                centre.addObserver(forName: UIApplication.didBecomeActiveNotification,
                                   object: nil, queue: .main) { [weak canvas] _ in
                    canvas?.didBecomeActive()
                },
            ]
        }

        deinit {
            for observer in observers { NotificationCenter.default.removeObserver(observer) }
        }
    }
}
