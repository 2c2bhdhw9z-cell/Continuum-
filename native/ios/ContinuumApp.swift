// Continuum — the Step 10 host app.
//
// Enough SwiftUI to prove the chain end to end: the C++ libretro wrapper renders a rotating
// colour, the Rust engine drives it and composites, and this presents the result on a
// CAMetalLayer. No library, no ROM picker — those are steps 11 and later. If the square is
// rotating and the HUD is counting frames, the whole Phase 5 boundary is working.

import SwiftUI
import UIKit

@main
struct ContinuumApp: App {
    var body: some Scene {
        WindowGroup {
            StubHarnessView()
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

    private var started = false

    /// The stub wrapper's own numbers, from `native/switch-wrapper/stub_engine.h` and the
    /// `ScreenInfo` the C++ reports through `retro_get_system_av_info`. They have to agree:
    /// the declaration sizes the GPU texture, and the engine trusts it over the core.
    private enum Stub {
        static let coreId = "switch-stub"
        static let library = "libcontinuum_switch.dylib"
        static let width: UInt32 = 1280
        static let height: UInt32 = 720
        static let maxWidth: UInt32 = 1920
        static let maxHeight: UInt32 = 1080
        static let fps = 60.0
        static let sampleRate: UInt32 = 48000
        // The stub's software renderer writes bytes in R,G,B,A order, so it is declared
        // RGBA8888 (2) and uploaded without conversion. Worth knowing that this is *not* one
        // of libretro's three software formats — a real core would use XRGB8888, which is
        // byte order B,G,R,X. The stub gets away with it because the same code produces and
        // checks the colour, and because its real purpose is the hardware path.
        static let pixelFormat: UInt32 = 2
    }

    init() {
        engine = ContinuumEngine()
    }

    /// Starts the session. Called only once the renderer exists.
    ///
    /// Ordering is not cosmetic: `launch` fails with `NoRenderer` if the surface has not been
    /// attached yet, and the attach happens in `layoutSubviews`. Kicking this off from
    /// `.task` — as this file used to — races that and loses roughly whenever layout is slow.
    func startSession() {
        guard !started else { return }

        guard let core = Bundle.main.privateFrameworksURL?
            .appendingPathComponent(Stub.library) else {
            status = "no Frameworks directory in the bundle"
            return
        }
        guard FileManager.default.fileExists(atPath: core.path) else {
            status = "\(Stub.library) is missing from the bundle"
            return
        }

        // Switch containers declare need_fullpath, so content is a path rather than bytes.
        // The stub needs no real content, but it is given a path so the same code path runs.
        let content = FileManager.default.temporaryDirectory
            .appendingPathComponent("continuum-stub.nro")
        FileManager.default.createFile(atPath: content.path, contents: Data())

        let systemDir = FileManager.default.urls(for: .applicationSupportDirectory,
                                                 in: .userDomainMask).first

        do {
            // Declared before loaded, always: `loadNativeCore` refuses an undeclared id
            // rather than inventing geometry for it.
            try engine.declareCore(
                declaration: CoreDeclaration(
                    id: Stub.coreId,
                    displayName: "Continuum Switch (stub)",
                    systems: ["switch"],
                    modulePath: core.path,
                    baseWidth: Stub.width,
                    baseHeight: Stub.height,
                    maxWidth: Stub.maxWidth,
                    maxHeight: Stub.maxHeight,
                    aspectRatio: 16.0 / 9.0,
                    targetFps: Stub.fps,
                    audioSampleRate: Stub.sampleRate,
                    pixelFormat: Stub.pixelFormat,
                    priority: 0
                )
            )
            try engine.loadNativeCore(
                coreId: Stub.coreId,
                libraryPath: core.path,
                systemDir: systemDir?.path,
                saveDir: systemDir?.path
            )
            try engine.launch(
                coreId: Stub.coreId,
                contentId: "stub",
                rom: Data(),
                filename: content.path
            )
            started = true
            status = "running"
        } catch {
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

struct StubHarnessView: View {
    @StateObject private var host = EngineHost()

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
                Text("Continuum · Phase 5 step 1 + 10")
                    .font(.system(.caption, design: .monospaced)).bold()
                Text(host.status)
                if !host.gpu.isEmpty {
                    Text(host.gpu)
                }
                Text("\(host.frameCount) frames · \(host.displayFps, specifier: "%.0f") fps"
                     + " · \(host.dropped) dropped")
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
