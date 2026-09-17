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
    @Published var status: String = "starting"

    init() {
        engine = ContinuumEngine()
    }

    /// Loads the stub wrapper and starts a session.
    ///
    /// The wrapper is `dlopen`ed from the bundle's Frameworks directory. Its `RPATH` must be
    /// `@executable_path/Frameworks` or this resolves in the simulator and fails on device —
    /// which is a confusing way to lose an afternoon.
    func launchStub() {
        guard let core = Bundle.main.privateFrameworksURL?
            .appendingPathComponent("libcontinuum_switch.dylib") else {
            status = "wrapper not found in the bundle"
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
            try engine.loadNativeCore(
                coreId: "switch-stub",
                libraryPath: core.path,
                systemDir: systemDir?.path,
                saveDir: systemDir?.path
            )
            try engine.launch(
                coreId: "switch-stub",
                contentId: "stub",
                rom: Data(),
                filename: content.path
            )
            status = "running"
        } catch {
            status = "\(error)"
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
            MetalCanvasView(engine: host.engine) { telemetry in
                host.frameCount = telemetry.frameCount
                host.displayFps = telemetry.displayFps
                host.dropped = telemetry.dropped
            }
            .ignoresSafeArea()

            // The HUD is the actual test result: a rotating colour with a frozen frame
            // counter means the compositor is fine and the gate is not releasing, which is
            // a different bug from a black screen.
            VStack(alignment: .leading, spacing: 4) {
                Text("Continuum · Phase 5 step 10")
                    .font(.system(.caption, design: .monospaced)).bold()
                Text(host.status)
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
        .task { host.launchStub() }
    }
}

/// Bridges `MetalCanvas` into SwiftUI, and wires the app lifecycle to it.
struct MetalCanvasView: UIViewRepresentable {
    let engine: ContinuumEngine
    let onTelemetry: (TickTelemetry) -> Void

    func makeUIView(context: Context) -> MetalCanvas {
        let canvas = MetalCanvas(engine: engine)
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
