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

import SwiftUI
import UIKit
import UniformTypeIdentifiers

@main
struct ContinuumApp: App {
    var body: some Scene {
        WindowGroup {
            PlayerView()
                // The player is always dark: it surrounds emulated output, and a light
                // letterbox around a dark game is glare rather than design.
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
    /// File size in bytes, or -1 when the size could not be read.
    let byteCount: Int64

    var id: String { path }

    init(url: URL) {
        path = url.path
        name = url.lastPathComponent
        ext = url.pathExtension.lowercased()
        var bytes: Int64 = -1
        if let values = try? url.resourceValues(forKeys: [.fileSizeKey]),
           let size = values.fileSize {
            bytes = Int64(size)
        }
        byteCount = bytes
    }

    /// Human-sized file size. Hand-rolled rather than routed through a formatter because this
    /// is a diagnostic read-out: it must not be localised and it must not vary by locale.
    var sizeText: String {
        if byteCount < 0 { return "size unknown" }
        let megabytes = Double(byteCount) / (1024.0 * 1024.0)
        if megabytes >= 1.0 {
            return String(format: "%.1f MB", megabytes)
        }
        let kilobytes = Double(byteCount) / 1024.0
        return String(format: "%.0f KB", kilobytes)
    }

    /// The secondary row line, built as a `String` so `Text` takes its verbatim initialiser.
    var detail: String {
        "\(ext.uppercased()) · \(sizeText)"
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

    @Published var frameCount: UInt64 = 0
    @Published var displayFps: Double = 0
    @Published var dropped: UInt32 = 0
    @Published var status: String = "waiting for the surface"
    @Published var gpu: String = ""
    /// The on-screen BIOS/HLE line. Populated the moment the system dir is known, so a
    /// missing BIOS is a legible condition rather than a silent drop in compatibility.
    @Published var bios: String = ""
    /// True once a ROM has been launched. Drives the HUD affordance label and re-entrancy.
    @Published var running = false

    /// The launchable games found in Documents, newest scan wins.
    @Published var library: [LibraryEntry] = []
    /// The Library's own status line, kept separate from `status` on purpose.
    ///
    /// An import writes its summary to `status`, and the rescan that follows must not wipe
    /// that summary out. Two independent lines mean the user reads both the import result and
    /// the state of the Library at the same time, which is the whole point of a HUD that is
    /// also the only debugger on a sideloaded build.
    @Published var libraryStatus: String = "library: not scanned yet"

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
    /// DO NOT "TIGHTEN" THIS INTO A LIST OF ROM TYPES. It was learned the hard way, twice.
    /// iOS ships no built-in `UTType` for .cue, .bin, .chd or .pbp, so
    /// `UTType(filenameExtension: "cue")` and friends return nil, and a picker whose
    /// `allowedContentTypes` is built from them GREYS THOSE EXACT FILES OUT: the user opens
    /// the picker, sees their game, and cannot tap it. Declaring the types properly would mean
    /// exporting custom UTIs from the Info.plist, which is more moving parts than a sideloaded
    /// build needs.
    ///
    /// So the picker stays permissive and NOTHING is greyed out, and acceptance is enforced
    /// AFTER the pick, by extension, in `importFiles`. Rejected files are named on the HUD
    /// along with their extension, so a wrong pick is legible rather than mysterious.
    /// Directories nominally conform to `public.item` too; one picked by accident has no
    /// importable extension and is reported by that same rejection path.
    private static let pickerContentTypes: [UTType] = [UTType.item]

    /// Extensions that may be copied into Documents.
    ///
    /// Wider than the launchable set below because a CD game is a set of files: the .bin
    /// tracks must come in alongside the .cue that names them, even though a .bin is never
    /// itself a launch target.
    private static let importableExtensions = ["bin", "chd", "cue", "iso", "pbp"]

    /// Extensions the Library offers as a launch target.
    ///
    /// This is all that is left of the old `.cue > .pbp > .iso > .chd > .bin` priority order,
    /// and it is the only place that knowledge is still meaningful. A .cue, .chd, .pbp or .iso
    /// is a thing the core can be pointed at; a .bin is a track that the core opens for itself
    /// when the .cue names it. So .bin files stay on disk, keep being read, and stay out of
    /// this list rather than cluttering it with entries that must not be tapped.
    private static let launchableExtensions = ["chd", "cue", "iso", "pbp"]

    /// How many filenames the HUD names before it starts counting instead. Enough to identify
    /// what happened, short enough to stay on screen.
    private static let reportedNameLimit = 6

    /// PCSX ReARMed's pre-load numbers. These are only a hint: `declareCore` sizes an
    /// initial GPU texture from them, but `sync_descriptor_from_core` overwrites geometry,
    /// fps and sample rate from `retro_get_system_av_info` the instant the core loads, and
    /// the pixel format is re-negotiated through `SET_PIXEL_FORMAT` inside `retro_load_game`.
    /// So the accuracy that matters is the filename and the core id, not these figures.
    private enum PS1 {
        // Must match the dylib filename produced by scripts/build-core.sh and embedded by
        // package-ipa.sh; a mismatch here is a "core is missing from the bundle" HUD line.
        static let coreId = "pcsx_rearmed"
        static let library = "pcsx_rearmed_libretro_ios.dylib"
        // PS1 native resolution is 320x240; the framebuffer can widen to 640x480 and, with
        // interlace and overscan, up to about 700x576. The declared max sizes the texture
        // before the core reports its real av_info.
        static let width: UInt32 = 320
        static let height: UInt32 = 240
        static let maxWidth: UInt32 = 700
        static let maxHeight: UInt32 = 576
        // PS1 runs at NTSC 59.94 Hz; the pacer takes the real number the core reports on
        // load, so this is only the pre-load hint.
        static let fps = 59.94
        static let sampleRate: UInt32 = 44100
        static let aspectRatio: Float = 4.0 / 3.0
        // RGB565 (0) is PCSX ReARMed's default. The core re-negotiates via SET_PIXEL_FORMAT
        // during retro_load_game (RGB565 by default, XRGB8888 if pcsx_rearmed_rgb32_output
        // is on), and native_core.rs's video() reports whatever the core chose, so this
        // declaration is superseded either way.
        static let pixelFormat: UInt32 = 0
        // A real BIOS placed in the system dir (e.g. scph1001.bin) raises compatibility.
        // These are the filenames PCSX ReARMed looks for; the HUD reports whether any of
        // them is present. Never bundled: shipping a PS1 BIOS is a copyright violation.
        static let biosNames = ["scph1001.bin", "scph5501.bin", "scph7001.bin",
                                "scph1000.bin", "scph5500.bin", "scph5502.bin"]
    }

    init() {
        engine = ContinuumEngine()
        // Scan up front so the Library is populated even if the Metal attach later fails.
        // An attach failure must not also hide the games the user already imported.
        refreshLibrary()
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

    /// Reports whether a PS1 BIOS is present in the system dir.
    ///
    /// PCSX ReARMed does not require one: with no BIOS it falls back to HLE (its
    /// `pcsx_rearmed_bios` option / `Config.HLE`) and still boots, at reduced accuracy. A
    /// missing BIOS is therefore a compatibility note, not a hard failure, so it belongs on
    /// the HUD rather than in an error path.
    private func biosStatus(in systemDir: URL?) -> String {
        guard let dir = systemDir else { return "no system dir; HLE BIOS only" }
        let present = PS1.biosNames.first { name in
            FileManager.default.fileExists(atPath: dir.appendingPathComponent(name).path)
        }
        if let present {
            return "BIOS: \(present)"
        }
        return "BIOS: none, HLE fallback"
    }

    /// The engine core states in which a session can actually be started.
    ///
    /// `"loaded"` is resident and idle. `"bound"` is resident and already checked out by a
    /// session, which is usable too: `engine.launch` stops the previous session as its first
    /// act, and that hands the core back to the registry. Reloading a bound core would tear
    /// down a session the engine is about to unwind properly by itself.
    private static let usableCoreStates = ["loaded", "bound"]

    /// Makes the PS1 core resident, and re-makes it resident whenever the engine has dropped it.
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
    /// So no bool is kept at all. The state is read back from the engine on every call, and the
    /// declare + load sequence is re-run whenever the core is not usable, which makes this path
    /// self-healing rather than one-shot. Re-declaring is safe to repeat: `CoreRegistry::declare`
    /// keeps an existing loaded instance and only refreshes metadata, so it cannot yank a core
    /// that is already resident.
    ///
    /// Declare + load only, never a launch: safe to run before any ROM exists, because PCSX
    /// ReARMed does not touch a game path until `retro_load_game`. Returns `false`, and leaves a
    /// specific reason on the HUD, if the core cannot be made resident.
    @discardableResult
    private func ensureCoreLoaded() -> Bool {
        // Ask the engine, never a local flag. `coreState` is Optional and returns nil when the id
        // is unknown to the registry, i.e. nothing has been declared yet, which needs the same
        // reload as an unloaded "declared". Collapsing that nil to "unknown" right here keeps one
        // non-optional string for both the comparison and the HUD, and "unknown" is never a
        // usable state, so an unknown id always takes the reload path below.
        let observed = engine.coreState(coreId: PS1.coreId) ?? "unknown"
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
            status = "core state is failed; retrying load of \(PS1.coreId)..."
        } else {
            status = "core state was \(observed); loading \(PS1.coreId)..."
        }

        guard let core = Bundle.main.privateFrameworksURL?
            .appendingPathComponent(PS1.library) else {
            status = "no Frameworks directory in the bundle"
            return false
        }
        guard FileManager.default.fileExists(atPath: core.path) else {
            status = "\(PS1.library) is missing from the bundle"
            return false
        }

        let systemDir = systemDirectory()
        bios = biosStatus(in: systemDir)

        do {
            // Declared before loaded, always: `loadNativeCore` refuses an undeclared id
            // rather than inventing geometry for it.
            try engine.declareCore(
                declaration: CoreDeclaration(
                    id: PS1.coreId,
                    displayName: "PCSX ReARMed (PS1)",
                    systems: ["ps1"],
                    modulePath: core.path,
                    baseWidth: PS1.width,
                    baseHeight: PS1.height,
                    maxWidth: PS1.maxWidth,
                    maxHeight: PS1.maxHeight,
                    aspectRatio: PS1.aspectRatio,
                    targetFps: PS1.fps,
                    audioSampleRate: PS1.sampleRate,
                    pixelFormat: PS1.pixelFormat,
                    priority: 0
                )
            )
            try engine.loadNativeCore(
                coreId: PS1.coreId,
                libraryPath: core.path,
                systemDir: systemDir?.path,
                saveDir: systemDir?.path
            )
        } catch {
            // The HUD is the only diagnostic on a sideloaded build, so the error text lands
            // there rather than throwing into a blank screen.
            status = "\(error)"
            return false
        }

        // Confirm against the engine rather than concluding success from "the two calls above
        // did not throw". Believing a local success signal over the registry is exactly the
        // mistake the cached bool made, so the result is verified where it actually lives.
        let reloaded = engine.coreState(coreId: PS1.coreId) ?? "unknown"
        guard Self.usableCoreStates.contains(reloaded) else {
            status = "core load did not take: state is \(reloaded)"
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
                includingPropertiesForKeys: [.fileSizeKey],
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
            .filter { Self.launchableExtensions.contains($0.pathExtension.lowercased()) }
            .map { LibraryEntry(url: $0) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        library = entries

        if entries.isEmpty {
            libraryStatus = contents.isEmpty
                ? "library: empty, no files in Documents yet"
                : "library: nothing launchable among \(contents.count) file(s) in Documents "
                    + "(need a .cue, .chd, .pbp or .iso)"
        } else {
            libraryStatus = "library: \(entries.count) game(s) of "
                + "\(contents.count) file(s) in Documents"
        }
    }

    /// Deletes the tapped-away rows from Documents, then rescans.
    ///
    /// Deliberately shallow: it removes the launch target that was swiped, and says out loud
    /// that any .bin tracks that target named are still on disk. Guessing which .bin files
    /// belonged to a deleted .cue would mean parsing the cue sheet, and a wrong guess deletes
    /// a track another game needs.
    func deleteEntries(at offsets: IndexSet) {
        var removed: [String] = []
        var failures: [String] = []

        for index in offsets where index >= 0 && index < library.count {
            let entry = library[index]
            do {
                try FileManager.default.removeItem(atPath: entry.path)
                removed.append(entry.name)
            } catch {
                failures.append("\(entry.name): \(error.localizedDescription)")
            }
        }

        if removed.isEmpty && failures.isEmpty {
            // Only reachable if the swipe resolved to nothing at all, which would mean the
            // list and the offsets had drifted apart. Say so rather than looking successful.
            status = "delete matched no library row; the list was rescanned"
        } else if failures.isEmpty {
            status = "deleted \(Self.nameList(removed)); "
                + "any .bin tracks it named are still in Documents"
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
            guard Self.importableExtensions.contains(ext) else {
                let extLabel = ext.isEmpty ? "no extension" : ".\(ext)"
                rejected.append("\(name) [\(extLabel)]")
                status = "skipped \(name): \(extLabel) is not PS1 content "
                    + "(want .cue, .bin, .chd, .pbp or .iso)"
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

    /// Launches a game the user tapped in the Library.
    ///
    /// The path is inside our own Documents directory, so there is NO security scope here and
    /// nothing to acquire, hold or release. That is the entire point of the import step: the
    /// C++ core opens the .cue and each adjacent .bin it references with a plain fopen, on
    /// files this app owns, and the OS has no grant to refuse. PCSX ReARMed declares
    /// need_fullpath, so it is handed empty ROM bytes and the real absolute path; the Rust
    /// side routes on the '/' in that path and splits the real extension for content-info, so
    /// `entry.path` is exactly what it needs and nothing about the Rust launch signature
    /// changed with this rework.
    func launch(entry: LibraryEntry) {
        // Breadcrumb, written before the re-entrancy stop and before ANY guard below can
        // return. A tap that reaches this method therefore always changes the HUD. If the HUD
        // ever stays on the previous line, this method provably was not reached, which narrows
        // the fault to the Library row rather than to anything in here.
        let opening = "opening \(entry.name)..."
        status = opening

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
        guard ensureCoreLoaded() else {
            // ensureCoreLoaded writes its own specific reason on every failure path. Belt
            // and braces: if the status is somehow still the breadcrumb, say plainly that
            // the core is not resident rather than leaving a line that reads like progress.
            if status == opening {
                status = "core not loaded: \(PS1.library) could not be made resident"
            }
            return
        }

        // Record the state the engine reports at the moment of launch, so a launch that still
        // fails cannot leave the core state as the one unobservable fact in the sequence. This
        // is the line that would have named "declared" out loud instead of costing device trips.
        status = "\(opening) core \(engine.coreState(coreId: PS1.coreId) ?? "unknown")"

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
                coreId: PS1.coreId,
                contentId: entry.name,
                rom: Data(),
                filename: entry.path
            )
            running = true
            status = "running: \(entry.name)"
        } catch {
            running = false
            status = "launch failed: \(entry.name): \(error)"
        }
    }

    /// Stops the current session.
    ///
    /// Nothing else to unwind. This used to also release the picked folder's security scope,
    /// which no longer exists anywhere in this file: imported content lives in our own sandbox,
    /// so there is no scope to balance and no half-wired lifecycle left behind.
    func stopSession() {
        if running {
            engine.stop()
            running = false
        }
    }

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
    private static func topmostViewController() -> UIViewController? {
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
            // Make the core resident up front so a Library tap launches instantly, but do NOT
            // launch anything: PCSX ReARMed needs a real game and hard-rejects empty content.
            if ensureCoreLoaded() {
                refreshLibrary()
                status = library.isEmpty
                    ? "surface ready - tap Import Games to add a PS1 game"
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

struct PlayerView: View {
    /// Owns the engine host for the app's lifetime, which is also what keeps the picker's
    /// delegate alive long enough to be called back. See `EngineHost.importPickerDelegate`.
    @StateObject private var host = EngineHost()

    /// Built as a `String`, not as an interpolated `Text` literal.
    ///
    /// `Text("...")` takes a `LocalizedStringKey`, two of which cannot be concatenated with
    /// `+`, which is what this line used to try, and why the app had never actually compiled.
    /// Handing `Text` a `String` selects the verbatim initialiser instead, which is also what
    /// a diagnostic read-out wants: nothing here should be run through localisation.
    private var stats: String {
        let fps = String(format: "%.0f", host.displayFps)
        return "\(host.frameCount) frames · \(fps) fps · \(host.dropped) dropped"
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            MetalCanvasView(
                engine: host.engine,
                onAttach: { host.surfaceAttached($0) },
                onTelemetry: { telemetry in
                    host.frameCount = telemetry.frameCount
                    host.displayFps = telemetry.displayFps
                    host.dropped = telemetry.dropped
                }
            )
            .ignoresSafeArea()

            // HUD above, Library below, both on screen at once and neither behind navigation.
            // The HUD is how every problem on this device gets diagnosed, so it does not get
            // traded away for a tidier Library.
            VStack(alignment: .leading, spacing: 8) {
                hud
                LibraryView(host: host)
            }
            .padding()
        }
        .background(.black)
    }

    /// The diagnostic panel. On a sideloaded build with no debugger this is the actual test
    /// result, and the only one. Each line distinguishes a different failure:
    ///   - no GPU line          -> attachMetal failed; the reason is in `status`
    ///   - GPU but 0 frames     -> renderer up, core or gate not producing
    ///   - frames but no colour -> compositor or pixel-format problem
    ///   - frozen counter       -> the frame gate is not releasing
    private var hud: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Continuum · Phase 5 step 2 - PCSX ReARMed (software)")
                .font(.system(.caption, design: .monospaced)).bold()
            Text(host.status)
                .fixedSize(horizontal: false, vertical: true)
            if !host.bios.isEmpty {
                Text(host.bios)
            }
            if !host.gpu.isEmpty {
                Text(host.gpu)
            }
            Text(stats)
        }
        .font(.system(.caption2, design: .monospaced))
        .foregroundStyle(.white)
        .padding(10)
        .background(.black.opacity(0.55), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// The Library: what has been imported into Documents, and the only way to start a game.
///
/// It lists launch targets only (.cue, .chd, .pbp, .iso). The .bin tracks a .cue names are on
/// disk beside it and the core reads them, but they are not tappable, because handing a raw
/// track to the core instead of its cue sheet is not something a user should be able to do by
/// accident.
struct LibraryView: View {
    @ObservedObject var host: EngineHost

    /// The empty state, which has to be guidance rather than a shrug: on this device there is
    /// nothing else to tell the user what to do next. It also mentions the free fallback that
    /// `UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace` already give us.
    private static let emptyGuidance =
        "No games yet. Tap Import Games, then select a .cue together with every .bin track it "
        + "names (in the picker: Select, tap each file, Open). Files you drop into the "
        + "Continuum folder in the Files app show up here too."

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text("Library")
                    .font(.system(.caption, design: .monospaced)).bold()
                Spacer()
                Button("Import Games") {
                    host.presentImportPicker()
                }
                .font(.system(.caption, design: .monospaced))
            }

            Text(host.libraryStatus)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Color.white.opacity(0.75))
                .fixedSize(horizontal: false, vertical: true)

            if host.library.isEmpty {
                Text(Self.emptyGuidance)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(Color.white.opacity(0.75))
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                List {
                    // Identity comes from `LibraryEntry.id`, the absolute path, so rows keep
                    // their identity across the rescans that follow every import and delete.
                    ForEach(host.library) { entry in
                        Button {
                            host.launch(entry: entry)
                        } label: {
                            LibraryRow(entry: entry)
                        }
                        .buttonStyle(.plain)
                        .listRowInsets(EdgeInsets(top: 6, leading: 4, bottom: 6, trailing: 4))
                        .listRowBackground(Color.white.opacity(0.06))
                        .listRowSeparatorTint(Color.white.opacity(0.15))
                    }
                    .onDelete { offsets in
                        host.deleteEntries(at: offsets)
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .frame(maxHeight: 240)
            }
        }
        .padding(10)
        .background(.black.opacity(0.55), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// One Library row: the filename, and enough detail to tell two dumps of the same game apart.
struct LibraryRow: View {
    let entry: LibraryEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(entry.name)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.white)
            Text(entry.detail)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Color.white.opacity(0.7))
        }
        // Full-width and hit-testable across the whole row, so the tap target is the row
        // rather than just the glyphs of the filename.
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// Bridges `MetalCanvas` into SwiftUI, and wires the app lifecycle to it.
struct MetalCanvasView: UIViewRepresentable {
    let engine: ContinuumEngine
    let onAttach: (Result<String, Error>) -> Void
    let onTelemetry: (TickTelemetry) -> Void

    func makeUIView(context: Context) -> MetalCanvas {
        let canvas = MetalCanvas(engine: engine)
        canvas.onAttach = onAttach
        canvas.onTelemetry = onTelemetry
        canvas.start()
        context.coordinator.observe(canvas)
        return canvas
    }

    func updateUIView(_ canvas: MetalCanvas, context: Context) {
        canvas.onTelemetry = onTelemetry
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
