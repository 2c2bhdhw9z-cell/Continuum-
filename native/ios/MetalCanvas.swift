// Continuum: the one drawing surface, and the one MTLDevice.
//
// The architectural rule is unchanged from the design document: **there is exactly one
// MTLDevice in this process.** An iPhone has one GPU, Metal resources belong to the device
// that created them, and if wgpu, MoltenVK and ANGLE each made their own, no texture would
// be shareable and every frame would need a copy.
//
// What changed is *who creates it*. The blueprint had this file create the device and hand
// it to the engine. That is not implementable on wgpu 30, and worse than that, it would have
// looked like it worked:
//
//   - wgpu's Metal backend has no public constructor that accepts an existing MTLDevice.
//     The only path from a device to an adapter is private.
//   - wgpu's `Surface::configure` calls `CAMetalLayer.setDevice` with *its own* device. So
//     a device assigned here is replaced the moment the engine configures the surface, and
//     this file would be left holding a device that owns nothing the layer draws.
//
// So the device travels the other way: the engine creates it, and this file reads it back
// with `metalDeviceHandle()`. Nothing here calls MTLCreateSystemDefaultDevice(). The
// reasoning, with source references, is in crates/emulator-bridge/src/gfx/metal.rs.
//
// Because the engine owns the surface, it also owns almost all of the layer's
// configuration: device, pixelFormat, framebufferOnly, colorspace, maximumDrawableCount,
// opacity and drawableSize are all set by `configure`. Setting them here would be
// misleading, because they would be overwritten on the first frame. Only the properties wgpu
// leaves alone are set below.

import Metal
import QuartzCore
import UIKit

/// A UIView backed by CAMetalLayer, plus the display link that drives the engine.
final class MetalCanvas: UIView {
    /// The process's MTLDevice, read back from the engine after `attachMetal`.
    ///
    /// Not used for drawing, since the engine composites, but it is the handle MoltenVK will be
    /// initialised on when the Vulkan core path lands, and its `name` is the cheapest proof
    /// that the graphics chain came up on a real GPU.
    private(set) var device: MTLDevice?

    /// The engine's MTLCommandQueue. Anything Swift ever submits must use this one, so that
    /// submissions are ordered against the compositor's rather than racing it.
    private(set) var commandQueue: MTLCommandQueue?

    private var displayLink: CADisplayLink?
    private let engine: ContinuumEngine
    private var attached = false

    /// Called once per presented frame with the engine's telemetry, for the HUD.
    var onTelemetry: ((TickTelemetry) -> Void)?

    /// Supplies the ON-SCREEN pad's state for the frame about to run.
    ///
    /// Read here, in the display link, and not from the touch handlers, and that is the whole
    /// reason a held button stays held. `apply_gamepad_from` REPLACES one source layer wholesale on
    /// every call, because a poll is a complete statement about a device. Pushing only on touch-down
    /// would therefore have the very next frame clear the press: a button would fire once and let
    /// go by itself, which is unplayable in a way that looks like a flaky screen rather than a
    /// missing call.
    ///
    /// Nil until a player screen is on screen, and a released pad while no game runs.
    var gamepadSource: (() -> PadFrame)?

    /// Supplies every attached PHYSICAL controller's state for the frame about to run.
    ///
    /// A second source, on a second engine layer, and the separation is the point rather than
    /// tidiness. `gamepadSource` above is polled every frame whether or not a finger is on the
    /// glass, so if both fed the same layer the overlay's "nothing held" would erase a real
    /// controller sixty times a second, and the symptom would be a controller that works only
    /// while no thumb is near the screen. See the header of PhysicalControllers.swift.
    ///
    /// An empty array means nothing is attached, which is not the same as a released frame: with
    /// no controller there is nothing for the gamepad layer to be told.
    var controllerSource: (() -> [ControllerFrame])?

    /// The device audio path, pumped from the tick below.
    ///
    /// Pushed from here rather than pulled by the audio thread, and that is the whole design:
    /// `AVAudioSourceNode`'s render block runs on a real-time thread, the engine is a
    /// `Mutex<EmulatorBridge>`, and this tick holds that mutex for its entire duration. A render
    /// block reaching for the same lock would be a real-time thread waiting on the main thread,
    /// heard as clicks and dropouts rather than seen as a stall. So the drain happens here and
    /// the render block reads a lock-free Swift ring. See `AudioOutput`.
    ///
    /// Owned by `EngineHost`, not by this view, because SwiftUI may rebuild the view at any time
    /// and the audio graph must outlive that.
    var audio: AudioOutput?

    /// Reports how attaching went, so the harness can show it instead of a black screen.
    var onAttach: ((Result<String, Error>) -> Void)?

    override class var layerClass: AnyClass { CAMetalLayer.self }

    private var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    init(engine: ContinuumEngine) {
        self.engine = engine
        super.init(frame: .zero)
        configureLayer()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    private func configureLayer() {
        // Presented as soon as the frame is ready rather than waiting for the next Core
        // Animation transaction. wgpu does not touch this.
        metalLayer.presentsWithTransaction = false

        // Everything else that used to be set here (device, pixelFormat, framebufferOnly,
        // colorspace and maximumDrawableCount) is set by the engine's `Surface::configure`.
        //
        // One intent was lost in the move and is worth naming rather than pretending
        // otherwise: this file used to pin `maximumDrawableCount = 2`, on the argument that
        // a third drawable buys throughput an emulator does not need at the cost of a frame
        // of latency. wgpu derives it as `desired_maximum_frame_latency + 1`, and the
        // renderer asks for 2, so the layer ends up with 3. Changing that means changing
        // the shared renderer's configuration for the browser too, so it is a measurement
        // to make on a device rather than a guess to encode here.
    }

    // MARK: - Sizing

    override func layoutSubviews() {
        super.layoutSubviews()
        syncDrawableSize()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        syncDrawableSize()
    }

    /// Keeps the drawable in device pixels rather than points.
    ///
    /// Without `nativeScale` the image is soft on every device since the 6 Plus, because the
    /// layer would be rendered at point resolution and then upscaled by the compositor.
    private func syncDrawableSize() {
        let scale = window?.screen.nativeScale ?? UIScreen.main.nativeScale
        metalLayer.contentsScale = scale

        let size = CGSize(width: bounds.width * scale, height: bounds.height * scale)
        guard size.width > 0, size.height > 0 else { return }

        // Set before attaching so the first `configure` sees a sane extent. The engine sets
        // it again from the width/height passed below, to the same values.
        if size != metalLayer.drawableSize {
            metalLayer.drawableSize = size
        }

        let width = UInt32(size.width)
        let height = UInt32(size.height)

        if !attached {
            attachEngine(width: width, height: height)
        } else {
            engine.resizeSurface(width: width, height: height)
        }
    }

    /// Hands the engine the layer, then reads the Metal objects it created.
    ///
    /// `passUnretained` is correct for the layer: it is owned by this view as its backing
    /// layer, which outlives the engine's use of it. Passing retained would leak it.
    private func attachEngine(width: UInt32, height: UInt32) {
        let layerPointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(metalLayer).toOpaque()))

        do {
            try engine.attachMetal(layer: layerPointer, width: width, height: height)
            attached = true
            adoptEngineMetalObjects()

            let summary = engine.rendererSummary() ?? "renderer attached"
            NSLog("[continuum] %@", summary)
            onAttach?(.success(summary))
        } catch {
            // Reported rather than fatal: the engine names the stage that failed, and a
            // crash here would bury it under a stack trace inside Metal. On a sideloaded
            // build with no debugger attached, that message is the only diagnostic there is.
            NSLog("[continuum] attachMetal failed: \(error)")
            onAttach?(.failure(error))
        }
    }

    /// Reads the engine's MTLDevice and MTLCommandQueue back across the boundary.
    ///
    /// `takeUnretainedValue` because Rust owns both for the renderer's lifetime; Swift's own
    /// strong reference from here is an extra retain, which is harmless and makes the
    /// ordering between the two sides one less thing to get wrong.
    private func adoptEngineMetalObjects() {
        device = Self.object(from: engine.metalDeviceHandle()) as? MTLDevice
        commandQueue = Self.object(from: engine.metalQueueHandle()) as? MTLCommandQueue

        if device == nil {
            // Not fatal, but it means the invariant this file exists to enforce is broken:
            // the engine did not land on the Metal backend, or handed back a null.
            NSLog("[continuum] warning: engine reported no MTLDevice")
        }
    }

    private static func object(from handle: UInt64) -> AnyObject? {
        guard handle != 0, let raw = UnsafeRawPointer(bitPattern: UInt(handle)) else { return nil }
        return Unmanaged<AnyObject>.fromOpaque(raw).takeUnretainedValue()
    }

    // MARK: - The frame loop

    /// One CADisplayLink, driving the engine. The peer of the web build's single
    /// requestAnimationFrame loop, and the same rule: there is only ever one.
    func start() {
        guard displayLink == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        // Let the system choose within a range rather than pinning 60: a ProMotion display
        // can run 120 and a thermally throttled device cannot hold 60, and the pacer handles
        // a variable interval already because it takes a timestamp.
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 30, maximum: 120, preferred: 60)
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    func stop() {
        displayLink?.invalidate()
        displayLink = nil
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard attached else { return }

        // Input FIRST, then the step. `EmulatorBridge::tick` snapshots the merged pad state once
        // and hands that snapshot to every catch-up step of this tick, so a frame pushed after the
        // tick would not be seen until the next one. One frame of avoidable lag, on every press.
        //
        // Both sources are pushed here, and each names its OWN layer. `applyGamepadFrom` replaces
        // the layer it is given and merges with the others only when the core reads input, which is
        // what lets a thumb on the overlay and a real controller be held in the same frame, even on
        // the same button. Sending both to one layer, which is what the older
        // `applyGamepad(port:buttons:axes:)` would do, is the bug that reads as a broken controller.
        // The order of the two pushes does not matter, because they touch different layers; what
        // matters is that both land before the step.
        if let gamepadSource {
            let pad = gamepadSource()
            engine.applyGamepadFrom(port: 0, source: .touch, buttons: pad.buttons, axes: pad.axes)
            // The stylus rides the same frame as the buttons, on the same layer, because on a DS a
            // tap and a button press are frequently one action and splitting them across two ticks
            // would be a frame of skew between halves of the same input. Pushed unconditionally
            // rather than only while pressed, so the release is delivered too: a stroke that ended
            // must be seen to end, or the DS goes on believing the screen is held.
            engine.applyPointer(port: 0,
                                source: .touch,
                                x: Float(pad.pointer.x),
                                y: Float(pad.pointer.y),
                                pressed: pad.pointerPressed)
        }
        if let controllerSource {
            for pad in controllerSource() {
                engine.applyGamepadFrom(port: pad.port, source: .gamepad,
                                        buttons: pad.buttons, axes: pad.axes)
            }
        }

        // targetTimestamp, not timestamp: the pacer wants when this frame will be *shown*,
        // not when the callback fired. FramePacer::plan() takes a timestamp for exactly this
        // reason: it never reads a clock of its own.
        let telemetry = engine.tick(nowMillis: link.targetTimestamp * 1000.0)

        // Audio AFTER the step, and before the telemetry callback. After, because the samples
        // this tick's core steps just produced are the ones worth having and draining first
        // would always be a frame behind. Before the callback, so the HUD reads a ring depth
        // that is current rather than one drain stale.
        audio?.pump()

        onTelemetry?(telemetry)
    }

    // MARK: - Lifecycle

    /// Backgrounding: the MTLDevice survives, drawables do not.
    ///
    /// Ordering matters and is easy to get wrong. The checkpoint has to be written *before*
    /// the graphics context is torn down, because several cores keep emulated GPU state in
    /// host GPU resources and a state saved afterwards is quietly incomplete.
    func willResignActive() {
        engine.pause()
    }

    func didEnterBackground() {
        engine.releaseGraphics()
        stop()
        // After `stop()`, deliberately. The display link is what feeds the audio ring, so once
        // it is gone the render block would emit an unbroken run of silence and count an
        // underrun for every callback. Suspending is not the same as stopping: `AudioOutput`
        // remembers that a game wanted audio, so foregrounding brings it back.
        audio?.suspend()
    }

    func willEnterForeground() {
        syncDrawableSize()
        engine.restoreGraphics()
    }

    func didBecomeActive() {
        start()
        engine.resume(nowMillis: CACurrentMediaTime() * 1000.0)
        // Rebuilt rather than unpaused, inside `resume`: the route may have changed while the
        // app was away, and with it the rate the graph was built for.
        audio?.resume()
    }
}
