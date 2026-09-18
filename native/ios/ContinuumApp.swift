// Continuum — the Phase 5 Step 2 host app.
//
// Step 10 proved the chain with a stub: the C++ libretro wrapper rendered a rotating colour,
// the Rust engine drove it and composited, and this presented the result on a CAMetalLayer.
// Step 2 swaps that stub for the first real libretro core, PCSX ReARMed (PS1), through the
// same software frame path. If the HUD is counting frames, the loader, the pixel-format
// negotiation and the audio pipeline are all working against a real core.
//
// The core is loaded exactly as the stub was: declare, load, launch, in that order, from the
// attach callback so the renderer exists first. What changed is the core it points at, the
// PS1 geometry it declares, and that the launch filename is now a real, openable path
// because PCSX ReARMed declares need_fullpath and hard-rejects a NULL info->path.

import SwiftUI
import UIKit

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

    private var started = false

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

    /// Starts the session. Called only once the renderer exists.
    ///
    /// Ordering is not cosmetic: `launch` fails with `NoRenderer` if the surface has not been
    /// attached yet, and the attach happens in `layoutSubviews`. Kicking this off from
    /// `.task` — as this file used to — races that and loses roughly whenever layout is slow.
    func startSession() {
        guard !started else { return }

        guard let core = Bundle.main.privateFrameworksURL?
            .appendingPathComponent(PS1.library) else {
            status = "no Frameworks directory in the bundle"
            return
        }
        guard FileManager.default.fileExists(atPath: core.path) else {
            status = "\(PS1.library) is missing from the bundle"
            return
        }

        let systemDir = systemDirectory()
        bios = biosStatus(in: systemDir)

        // PCSX ReARMed declares need_fullpath, so content is a real openable path rather
        // than bytes: the core opens and reads the file itself and hard-rejects a NULL
        // info->path. No commercial ROM ships in the bundle, so a placeholder is written to
        // a real path in the writable area and passed as the launch filename with empty rom
        // bytes. CI cannot boot a game regardless; the point here is that the loader and the
        // fullpath plumbing run end to end. On device, dropping a real .cue/.bin/.pbp/.chd
        // at this path would boot it.
        let content = FileManager.default.temporaryDirectory
            .appendingPathComponent("continuum-ps1.cue")
        FileManager.default.createFile(atPath: content.path, contents: Data())

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
            try engine.launch(
                coreId: PS1.coreId,
                contentId: "ps1",
                rom: Data(),
                filename: content.path
            )
            started = true
            status = "running"
        } catch {
            // A load failure here (no ROM, unreadable path) is expected on a bare CI build
            // and must read legibly rather than blank the screen. The HUD carries the error.
            status = "\(error)"
        }
    }

    func surfaceAttached(_ result: Result<String, Error>) {
        switch result {
        case .success(let summary):
            gpu = summary
            status = "surface ready"
            startSession()
        case .failure(let error):
            status = "attach failed: \(error)"
        }
    }
}

private extension FileManager {
    func createFile(atPath path: String, contents: Data) {
        if !fileExists(atPath: path) {
            createFile(atPath: path, contents: contents, attributes: nil)
        }
    }
}

// MARK: - The view

struct PlayerView: View {
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
