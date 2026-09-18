// Continuum — the Phase 5 Step 2 host app.
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
// "retro_load_game rejected the content". So the host no longer auto-boots. The surface
// attach only makes the core resident (declare + load); the actual launch waits for the
// user to pick a real ROM through the Files app, and then hands the core the ROM's real
// absolute path.

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

/// A `Sendable` snapshot of the folder picker's result.
///
/// `UIDocumentPickerDelegate`'s methods carry no actor isolation that the compiler can see, so
/// what they receive has to cross a hop to reach the `@MainActor` engine host. Only Sendable
/// data travels: any failure is flattened to its finished HUD line right where it is received,
/// because `any Error` is not `Sendable`. `URL` is `Sendable`, so the picked folder passes
/// through untouched, which is what lets the delegate hand its security-scoped folder URL to
/// the main actor unchanged.
struct PickerOutcome: Sendable {
    /// Exactly what the picker returned, in order. Empty when the picker failed.
    let urls: [URL]
    /// Nil on success. On failure this is the HUD line, already built.
    let failureText: String?
}

// MARK: - The engine, owned once

/// Holds the engine for the app's lifetime.
///
/// One instance, created once. The engine is `Send + Sync` on the Rust side — it is a
/// `Mutex<EmulatorBridge>` — so it is safe to reach from any actor, which is exactly why
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

    /// Whether declare + load has already succeeded. The core is made resident once, on the
    /// first surface attach; a later ROM pick reuses it rather than re-declaring.
    private var coreLoaded = false
    /// The security-scoped URL of the FOLDER whose game is currently in play. PCSX ReARMed
    /// uses need_fullpath and keeps the files open for the whole session, and it opens the
    /// .cue's adjacent .bin tracks itself, so the scope must cover the whole folder subtree
    /// and stay open for the session's lifetime. It is balanced only when the session is
    /// replaced or stopped, never in a `defer` right after launch.
    private var activeScopedURL: URL?

    /// The folder picker's delegate, retained here for the app's lifetime.
    ///
    /// THIS PROPERTY IS THE FIX. `UIDocumentPickerViewController.delegate` is a WEAK
    /// reference, so the delegate object must be owned by something else for as long as the
    /// picker is on screen. A delegate created as a local inside the presenting function is
    /// released the moment that function returns, the picker's weak reference goes nil, and
    /// the callback never fires: the sheet then dismisses and nothing happens, which is
    /// exactly the symptom being fixed here. This host is owned by a `@StateObject` on the
    /// root view of the only `WindowGroup`, so it lives as long as the app does, and this
    /// strong reference therefore provably outlives any picker it is handed to.
    private lazy var folderPickerDelegate = FolderPickerDelegate(host: self)

    /// The picker that is currently on screen, retained for as long as it is up.
    ///
    /// UIKit owns a presented view controller, so this is belt and braces against the one
    /// remaining invisible failure: if anything releases or tears the picker down early, it
    /// disappears without a word and its delegate is never called, which reads on the HUD
    /// exactly like a delegate that refuses to fire. Holding a strong reference here means an
    /// early release cannot be what happened, so the HUD's silence has to mean something else.
    /// Cleared through `releaseActivePicker()` the moment any callback reports back.
    private var activePicker: UIDocumentPickerViewController?

    /// The content types the picker offers: a single FOLDER. Declared once, here, because this
    /// is the only place a picker is built.
    ///
    /// A CD-based game is a set of files (a .cue text sheet plus the .bin tracks it names), and
    /// picking the .cue alone scopes only that one file, so iOS then denies the C++ core's
    /// `fopen` of the adjacent .bin tracks and cue/bin games fail. Picking the FOLDER instead
    /// grants a security scope over the whole subtree, so the core can open the .cue AND every
    /// adjacent .bin. `UTType.folder` is a valid system type (no exported type declaration
    /// needed), so the directory is selectable while its non-folder siblings are greyed out,
    /// which is the intended affordance: pick the folder that holds the .cue and its tracks.
    private static let allowedTypes: [UTType] = [UTType.folder]

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
        // The ROM extensions PCSX ReARMed accepts that the picker should surface. These have
        // no standard system UTType, so they are built from the extension at runtime.
        static let romExtensions = ["chd", "pbp", "cue", "iso"]
    }

    init() {
        engine = ContinuumEngine()
    }

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

    /// Makes the PS1 core resident without launching any content.
    ///
    /// Declare + load only. This is safe to run before any ROM exists — PCSX ReARMed does
    /// not touch a game path until `retro_load_game`. Called once from the attach callback
    /// so the core is ready the instant a ROM is picked; returns `false` (and leaves an
    /// error on the HUD) if the core cannot be made resident.
    @discardableResult
    private func ensureCoreLoaded() -> Bool {
        if coreLoaded { return true }

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
            coreLoaded = true
            return true
        } catch {
            // The HUD is the only diagnostic on a sideloaded build, so the error text lands
            // there rather than throwing into a blank screen.
            status = "\(error)"
            return false
        }
    }

    /// The extension priority used to pick the launch entry from a picked folder.
    ///
    /// A CD-based game is a set of files: a .cue text sheet that names one or more .bin
    /// track files, or a single-file .pbp/.iso/.chd image. The .cue (or the single-file
    /// image) is what the core is handed; the .bin tracks are opened by the core itself as
    /// the .cue references them. So the launch entry is preferred in this order:
    ///   .cue  the CD descriptor that references adjacent .bin tracks (most common)
    ///   .pbp  a self-contained PSP/PS1 package
    ///   .iso  a single-file image
    ///   .chd  a compressed image (the core builds link against libchdr)
    ///   .bin  last resort, only when no descriptor exists (a raw single-track image)
    private static let launchExtensionPriority = ["cue", "pbp", "iso", "chd", "bin"]

    /// Launches the CD/game whose folder was picked through the Files app.
    ///
    /// The URL comes from `UIDocumentPickerViewController` as a FOLDER and points outside the
    /// app sandbox, so it is only readable inside its security scope. The picker is built with
    /// `asCopy: false` precisely so this is the ORIGINAL folder rather than a temp copy, which
    /// is what makes the scope below grant access to the real .cue and its sibling tracks.
    /// A single-file pick would scope only the picked .cue, and iOS would then deny the C++
    /// core's `fopen` of the adjacent .bin track files it references, which is exactly why
    /// cue/bin games failed. Scoping the FOLDER
    /// grants the whole subtree, so the core can open the .cue AND every adjacent .bin. The
    /// scope is opened here and held in `activeScopedURL` for the session's lifetime. It is
    /// released in `stopSession()` when the session ends or is replaced, never immediately
    /// after launch, because PCSX ReARMed keeps the files open for the whole session
    /// (need_fullpath).
    func launch(url: URL) {
        // Breadcrumb, written before the re-entrancy stop and before ANY guard below can
        // return. A pick that reaches this method therefore always changes the HUD. If the
        // HUD ever stays on the pre-pick line again, this method provably was not reached,
        // which narrows the fault to the picker itself rather than anything in here.
        let opening = "opening \(url.lastPathComponent)..."
        status = opening

        // Re-entrancy: a fresh pick replaces any running session and its scoped folder.
        if running || activeScopedURL != nil {
            stopSession()
        }

        guard ensureCoreLoaded() else {
            // ensureCoreLoaded writes its own specific reason on every failure path. Belt
            // and braces: if the status is somehow still the breadcrumb, say plainly that
            // the core is not resident rather than leaving a line that reads like progress.
            if status == opening {
                status = "core not loaded: \(PS1.library) could not be made resident"
            }
            return
        }

        // The security scope MUST be opened BEFORE the folder is enumerated below. Outside
        // its scope this URL reads as empty or unreadable, so enumerating first would report
        // a bogus "no ROM found" for a folder that actually holds the game. Do not reorder
        // the scope acquisition and the enumeration that follows it.
        guard url.startAccessingSecurityScopedResource() else {
            status = "cannot access \(url.path): security scope denied"
            return
        }
        // Hold the folder scope open for the session; do NOT stop it here. Every early
        // return past this line goes through `failAfterScope` so the scope cannot leak.
        activeScopedURL = url

        // Sanity check the shape of what came back. The picker is configured for folders, but
        // the user reported navigating INTO a folder before tapping Open, so report what the
        // URL actually is instead of enumerating a file and blaming a missing ROM.
        // Written as an explicit unwrap rather than a chained one-liner: `isDirectory` is
        // itself `Bool?`, so a chain off `try?` invites a nested optional. Absent or
        // unreadable resource values are treated as "not a directory" and reported.
        var pickedIsDirectory = false
        if let values = try? url.resourceValues(forKeys: [.isDirectoryKey]) {
            pickedIsDirectory = values.isDirectory ?? false
        }
        guard pickedIsDirectory else {
            failAfterScope(url, "picked a file, not a folder: \(url.lastPathComponent)")
            return
        }

        // Enumerate the folder and pick the launch entry by extension priority. The entry is
        // the file handed to the core; adjacent .bin tracks are reachable because the whole
        // folder is scoped.
        switch selectLaunchEntry(in: url) {
        case .unreadable(let error):
            // The folder could not be listed at all. Distinct from "nothing loadable in it":
            // this is a permission or I/O fault, and the real error text is the diagnostic.
            failAfterScope(url,
                           "cannot read \(url.lastPathComponent): "
                            + "\(error.localizedDescription) [\(error)]")

        case .noCandidate(let fileCount, let names):
            // The folder listed fine but held nothing loadable. Naming what WAS found turns
            // this from a dead end into an answer: wrong folder, nested subfolder, or an
            // extension the core does not take.
            let found = names.isEmpty ? "none" : names.joined(separator: ", ")
            failAfterScope(url,
                           "no .cue/.pbp/.iso/.chd/.bin in \(url.lastPathComponent) "
                            + "(\(fileCount) files: \(found))")

        case .entry(let entry):
            do {
                // need_fullpath: pass empty rom bytes and the real absolute path. The Rust
                // side routes on the '/' in the path and splits the real extension for
                // content-info, so the scoped entry file's `path` is exactly what it needs.
                try engine.launch(
                    coreId: PS1.coreId,
                    contentId: entry.lastPathComponent,
                    rom: Data(),
                    filename: entry.path
                )
                running = true
                status = "running: \(entry.lastPathComponent) "
                    + "(folder: \(url.lastPathComponent))"
            } catch {
                // Launch failed: release the folder scope we took so we do not leak it, and
                // put the error on the HUD.
                failAfterScope(url, "launch failed: \(entry.lastPathComponent): \(error)")
            }
        }
    }

    /// Reports a failure that happened AFTER the folder scope was taken.
    ///
    /// Balances the single `startAccessingSecurityScopedResource` of this pick, clears
    /// `activeScopedURL` so `stopSession` cannot release it twice, and puts the reason on the
    /// HUD. Every post-scope early return in `launch` goes through here, which is what keeps
    /// the scope open/release count matched at exactly one per successful pick.
    private func failAfterScope(_ url: URL, _ message: String) {
        url.stopAccessingSecurityScopedResource()
        activeScopedURL = nil
        running = false
        status = message
    }

    /// The three distinguishable results of inspecting a picked folder.
    ///
    /// This exists because the two failures are NOT the same condition and must not read the
    /// same on the HUD: a folder that cannot be listed (permissions, I/O) is a different
    /// problem from a folder that lists fine but holds nothing the core can load. The old
    /// `URL?` return collapsed both into nil, so an enumeration error was reported as
    /// "no ROM found", which sent the diagnosis in the wrong direction.
    private enum LaunchEntryOutcome {
        /// The file to hand the core.
        case entry(URL)
        /// The folder listed, but no entry matched the launch extensions. Carries the total
        /// number of visible files and a sample of their names for the HUD.
        case noCandidate(fileCount: Int, names: [String])
        /// The folder could not be listed at all; carries the real underlying error.
        case unreadable(Error)
    }

    /// How many of the folder's filenames the HUD names when nothing loadable was found.
    /// Enough to identify the folder, short enough to stay on screen.
    private static let reportedNameLimit = 6

    /// Chooses the file to hand the core from the contents of a picked folder.
    ///
    /// Enumerates the folder (non-recursive, skipping hidden files) and returns the highest
    /// priority entry by extension order (.cue > .pbp > .iso > .chd > .bin), matched
    /// case-insensitively. When several files share the winning extension, the choice is
    /// deterministic: the candidates are sorted by `lastPathComponent` and the first is taken.
    ///
    /// The caller must already hold the folder's security scope; see `launch`.
    private func selectLaunchEntry(in folder: URL) -> LaunchEntryOutcome {
        let contents: [URL]
        do {
            contents = try FileManager.default.contentsOfDirectory(
                at: folder,
                includingPropertiesForKeys: nil,
                options: [.skipsHiddenFiles]
            )
        } catch {
            // Propagated rather than swallowed by `try?`: the real error text is the only
            // thing that tells a sideloaded build apart from an empty folder.
            return .unreadable(error)
        }

        for ext in Self.launchExtensionPriority {
            let matches = contents
                .filter { $0.pathExtension.lowercased() == ext }
                .sorted { $0.lastPathComponent < $1.lastPathComponent }
            if let first = matches.first {
                return .entry(first)
            }
        }

        return .noCandidate(
            fileCount: contents.count,
            names: contents.prefix(Self.reportedNameLimit).map { $0.lastPathComponent }
        )
    }

    /// Stops the current session and releases the picked folder's security scope.
    func stopSession() {
        if running {
            engine.stop()
            running = false
        }
        if let url = activeScopedURL {
            url.stopAccessingSecurityScopedResource()
            activeScopedURL = nil
        }
    }

    /// Presents the folder picker directly from the live UIKit hierarchy.
    ///
    /// This replaces SwiftUI's `.fileImporter`, which was proven on device not to call its
    /// completion closure at all for a folder selection: the sheet dismissed and the HUD, whose
    /// very first statement in that closure was an unconditional breadcrumb, did not change.
    /// SwiftUI's presentation machinery is therefore the thing that failed, so the picker is
    /// presented straight from the window's topmost view controller instead of being routed
    /// back through a SwiftUI sheet.
    func presentFolderPicker() {
        // Breadcrumb written BEFORE anything else, so the button press itself is observable
        // even if resolving a presenter fails. If the HUD never reaches this line, the button
        // action is not running at all.
        status = "presenting folder picker..."

        guard let presenter = Self.topmostViewController() else {
            status = "cannot present picker: no root view controller"
            return
        }

        // asCopy: false is required. The core needs the ORIGINAL folder so its C++ side can
        // fopen the .cue and the sibling .bin tracks in place; a copy would land in a temp
        // location and is not valid for folders anyway.
        let picker = UIDocumentPickerViewController(
            forOpeningContentTypes: Self.allowedTypes,
            asCopy: false
        )
        // The weak delegate is pointed at the property that owns it, never at a local. See
        // `folderPickerDelegate` for why that distinction is the entire bug.
        picker.delegate = folderPickerDelegate
        picker.allowsMultipleSelection = false
        // Show extensions so the user can confirm by eye that the folder holds .cue/.bin.
        picker.shouldShowFileExtensions = true

        // Watches for a dismissal that yields no pick at all: a swipe-away, or something else
        // tearing the sheet down. Without this, "the user or the system closed the sheet" and
        // "the pick callback never fired" are the same blank HUD, which is the one ambiguity
        // still open. `presentationController` is created on demand from `modalPresentationStyle`
        // for a controller that has not been presented yet, so it is expected to be non-nil;
        // the pre-present line below reports whether the watch was actually wired rather than
        // leaving a silently unhooked callback.
        picker.presentationController?.delegate = folderPickerDelegate
        let dismissalWatch = picker.presentationController == nil
            ? "no dismissal watch"
            : "dismissal watch on"

        // Arm the delegate for this picker: clears the flag that suppresses the dismissal
        // notice, so a second pick attempt is tracked as carefully as the first.
        folderPickerDelegate.prepareForNewPicker()

        // Hold the picker so an early release cannot be the invisible cause of a silent HUD.
        activePicker = picker

        // Naming the presenter is the point of this line. If the picker is being presented
        // from something unexpected or already detached from the window, this is the smoking
        // gun, and it is only readable BEFORE the call in case the call itself never returns.
        let presenterName = String(describing: type(of: presenter))
        status = "presenting folder picker from \(presenterName) (\(dismissalWatch))..."

        // The completion handler is the fix to a LYING diagnostic. `present` is asynchronous,
        // so the old code's next-line "picker presented" claim was written whether or not the
        // presentation ever finished, which made "stuck waiting for the delegate" mean two
        // different things: a silent delegate, or a picker that never truly came up and took
        // the user's pick with a controller that was already gone. Only the completion handler
        // runs once UIKit has actually finished presenting, so only a HUD line written in here
        // can honestly claim the sheet is up.
        presenter.present(picker, animated: true) { [weak self] in
            // UIKit runs this on the main thread, but the closure carries no isolation the
            // compiler can see under SWIFT_VERSION 5.0, so the hop is explicit. `EngineHost`
            // is global-actor isolated and therefore Sendable, so a weak capture of it is
            // safe to carry across.
            Task { @MainActor in
                self?.status = "picker is up; waiting for a pick"
            }
        }
    }

    /// Drops the strong reference to the presented picker once it has reported back.
    ///
    /// Called from every delegate callback. Not private: `FolderPickerDelegate` is a separate
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

    /// Handles what the folder picker handed back, on the main actor.
    ///
    /// Split out of the delegate callback on purpose. `UIDocumentPickerDelegate`'s methods
    /// carry no actor isolation the compiler can see, so anything they do to this `@MainActor`
    /// object has to cross an actor hop first; mutating a `@Published` property from off the
    /// main actor may never reach the UI, which is one way a pick can dismiss the sheet and
    /// change nothing on screen. The delegate now only flattens what it received and hops here.
    func handlePicked(_ outcome: PickerOutcome) {
        // Unconditional breadcrumb: the first statement of the first main-actor code that runs
        // after a pick, ahead of every guard and early return. It proves the handler fired and
        // shows the raw shape of what came back. A failure line below replaces it immediately;
        // that is intended, because a specific error beats a breadcrumb.
        status = "picked \(outcome.urls.count) item(s): "
            + "\(outcome.urls.first?.lastPathComponent ?? "none")"

        if let failure = outcome.failureText {
            status = failure
            return
        }
        guard let url = outcome.urls.first else {
            status = "picker returned no folder"
            return
        }
        launch(url: url)
    }

    func surfaceAttached(_ result: Result<String, Error>) {
        switch result {
        case .success(let summary):
            gpu = summary
            // Make the core resident up front so a ROM pick launches instantly, but do NOT
            // launch anything: PCSX ReARMed needs a real game and hard-rejects empty content.
            if ensureCoreLoaded() {
                status = "surface ready - pick a PS1 ROM folder"
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
/// its `folderPickerDelegate` property, and `EngineHost` is itself owned by a `@StateObject` for
/// the app's lifetime. The reference back to the host is `unowned` so the two do not form a
/// retain cycle; it is safe because this object cannot outlive the host that owns it.
/// Both pick callbacks are implemented, and both name themselves on the HUD. The modern iOS 14+
/// array form is the one that should fire; the deprecated single-URL form is kept alongside it,
/// never instead of it, purely as a diagnostic. If the array selector genuinely is not being
/// delivered on this OS build, the single-URL one will fire and the HUD will say which, so the
/// question "is the selector wrong, or is the callback never sent at all" is answered by reading
/// the screen rather than by another round of guessing. Every callback is `@objc` explicitly:
/// these are optional protocol requirements, dispatched by selector from Objective-C, and none of
/// them should depend on the compiler inferring an `@objc` entry point.
///
/// `UIAdaptivePresentationControllerDelegate` is here for the same reason: it separates "the sheet
/// was closed without a pick" from "the pick callback stayed silent".
final class FolderPickerDelegate: NSObject, UIDocumentPickerDelegate,
                                  UIAdaptivePresentationControllerDelegate {
    private unowned let host: EngineHost

    /// Set as soon as any pick or cancel callback fires, and read only by the dismissal notice.
    ///
    /// The picker dismisses itself after a pick and after a cancel, so the dismissal callback can
    /// arrive on a perfectly successful run. Without this flag it would overwrite "running: ..."
    /// with "dismissed without a pick" and manufacture the exact false report this change exists
    /// to eliminate. Written synchronously on the thread UIKit calls back on, never inside the
    /// actor hop, so it is always set before any later callback can read it.
    private var outcomeDelivered = false

    init(host: EngineHost) {
        self.host = host
        super.init()
    }

    /// Re-arms dismissal tracking for a freshly built picker. Called by `presentFolderPicker`.
    func prepareForNewPicker() {
        outcomeDelivered = false
    }

    /// The modern iOS 14+ callback, and the one that is expected to fire. Its first act is a
    /// HUD write, before any processing.
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
            failureText: urls.isEmpty ? "delegate fired with 0 urls: nothing to open" : nil
        )
        deliver(fired, outcome)
    }

    /// The deprecated iOS 8 single-URL callback, kept ALONGSIDE the array form above.
    ///
    /// This is not the primary path and is not expected to fire. It exists so that the failure
    /// mode "the array selector is not being delivered" cannot hide: if this one runs, the HUD
    /// says so by name, and if neither runs then no pick callback is being sent at all and the
    /// signature was never the problem. It forwards into the SAME outcome and launch path with
    /// the single URL wrapped in an array, so there is exactly one downstream behaviour.
    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentAt url: URL) {
        let fired = "delegate fired (single-url): \(url.lastPathComponent)"
        deliver(fired, PickerOutcome(urls: [url], failureText: nil))
    }

    /// Distinguishes "the user backed out" from "the callback never fired".
    ///
    /// Without this line those two look identical on the HUD, which is exactly the ambiguity
    /// that made the previous two iterations impossible to diagnose.
    @objc func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        outcomeDelivered = true
        let host = self.host
        Task { @MainActor in
            host.status = "picker cancelled: no folder chosen"
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
    /// delegate alive long enough to be called back. See `EngineHost.folderPickerDelegate`.
    @StateObject private var host = EngineHost()

    /// Built as a `String`, not as an interpolated `Text` literal.
    ///
    /// `Text("...")` takes a `LocalizedStringKey`, two of which cannot be concatenated with
    /// `+` — which is what this line used to try, and why the app had never actually
    /// compiled. Handing `Text` a `String` selects the verbatim initialiser instead, which
    /// is also what a diagnostic read-out wants: nothing here should be run through
    /// localisation.
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

            // The HUD is the actual test result, and on a sideloaded build with no debugger
            // it is the only one. Each line distinguishes a different failure:
            //   - no GPU line          → attachMetal failed; the reason is in `status`
            //   - GPU but 0 frames     → renderer up, core or gate not producing
            //   - frames but no colour → compositor or pixel-format problem
            //   - frozen counter       → the frame gate is not releasing
            VStack(alignment: .leading, spacing: 4) {
                Text("Continuum · Phase 5 step 2 - PCSX ReARMed (software)")
                    .font(.system(.caption, design: .monospaced)).bold()
                Text(host.status)
                if !host.bios.isEmpty {
                    Text(host.bios)
                }
                if !host.gpu.isEmpty {
                    Text(host.gpu)
                }
                Text(stats)

                // Presents the UIKit picker directly. It no longer toggles a `@State` bool for
                // SwiftUI to act on, because SwiftUI's own presentation of the folder picker is
                // the thing that was proven not to call back.
                Button(host.running ? "Open another ROM Folder…" : "Open ROM Folder…") {
                    host.presentFolderPicker()
                }
                .font(.system(.caption, design: .monospaced))
                .padding(.top, 4)
            }
            .font(.system(.caption2, design: .monospaced))
            .foregroundStyle(.white)
            .padding(10)
            .background(.black.opacity(0.55), in: RoundedRectangle(cornerRadius: 8))
            .padding()
        }
        .background(.black)
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

    /// Lifecycle notifications, forwarded in the order §10 of the design document requires:
    /// checkpoint, then release graphics — never the other way round.
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
