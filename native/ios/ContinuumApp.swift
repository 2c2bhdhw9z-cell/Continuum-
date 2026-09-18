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
/// `.fileImporter`'s completion closure carries no actor isolation, so its result has to cross
/// a hop to reach the `@MainActor` engine host. `Result<[URL], any Error>` cannot make that hop
/// because `any Error` is not `Sendable`, so the error is flattened to its finished HUD line
/// right where it is received and only Sendable data travels. `URL` is `Sendable`, so the
/// picked folder passes through untouched.
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
    /// The URL comes from `.fileImporter` as a FOLDER and points outside the app sandbox, so
    /// it is only readable inside its security scope. A single-file pick would scope only the
    /// picked .cue, and iOS would then deny the C++ core's `fopen` of the adjacent .bin track
    /// files it references, which is exactly why cue/bin games failed. Scoping the FOLDER
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

    /// Handles what the folder picker handed back, on the main actor.
    ///
    /// Split out of the `.fileImporter` closure on purpose. That closure is a plain escaping
    /// closure with no actor isolation, so anything it does to this `@MainActor` object has to
    /// cross an actor hop first; mutating a `@Published` property from off the main actor may
    /// never reach the UI, which is one way a pick can dismiss the sheet and change nothing on
    /// screen. The closure now only flattens the result and hops here.
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

// MARK: - The view

struct PlayerView: View {
    @StateObject private var host = EngineHost()
    /// Drives the `.fileImporter` sheet. Toggled by the HUD affordance.
    @State private var importing = false

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

    /// The content types the picker offers: a single FOLDER.
    ///
    /// A CD-based game is a set of files (a .cue text sheet plus the .bin tracks it names), and
    /// picking the .cue alone scopes only that one file, so iOS then denies the C++ core's
    /// `fopen` of the adjacent .bin tracks and cue/bin games fail. Picking the FOLDER instead
    /// grants a security scope over the whole subtree, so the core can open the .cue AND every
    /// adjacent .bin. `UTType.folder` is a valid system type (no exported type declaration
    /// needed), so the directory is selectable while its non-folder siblings are greyed out,
    /// which is the intended affordance: pick the folder that holds the .cue and its tracks.
    private var allowedTypes: [UTType] {
        [UTType.folder]
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

                Button(host.running ? "Open another ROM Folder…" : "Open ROM Folder…") {
                    importing = true
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
        .fileImporter(
            isPresented: $importing,
            allowedContentTypes: allowedTypes,
            allowsMultipleSelection: false
        ) { result in
            // This closure is NOT main-actor isolated: `.fileImporter`'s completion is a plain
            // `@escaping (Result<[URL], Error>) -> Void`, and under SWIFT_VERSION 5.0 touching
            // the `@MainActor` host from here compiles but can run off the main actor, where a
            // write to a `@Published` property may never reach the HUD. So it does two things
            // only: flatten the result to Sendable data, and hop to the main actor. There is
            // no early return and no `guard` here, and the handler's first act is to write the
            // HUD, so a pick can no longer dismiss the sheet and leave the screen unchanged.
            let outcome: PickerOutcome
            switch result {
            case .success(let urls):
                outcome = PickerOutcome(urls: urls, failureText: nil)
            case .failure(let error):
                // Both forms are reported: localizedDescription is the readable one, the raw
                // error is the one that actually names an NSCocoaErrorDomain code.
                outcome = PickerOutcome(
                    urls: [],
                    failureText: "picker failed: \(error.localizedDescription) [\(error)]"
                )
            }
            Task { @MainActor [host] in
                host.handlePicked(outcome)
            }
        }
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
