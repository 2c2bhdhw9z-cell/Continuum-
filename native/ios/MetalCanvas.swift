// Continuum — the one drawing surface, and the one MTLDevice.
//
// The architectural rule is unchanged from the design document: **there is exactly one
// MTLDevice in this process.** An iPhone has one GPU, Metal resources belong to the device
// that created them, and if wgpu, MoltenVK and ANGLE each made their own, no texture would
// be shareable and every frame would need a copy.
//
// What changed is *who creates it*. The blueprint had this file create the device and hand
// it to the engine. That is not implementable on wgpu 30, and — worse — it would have
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
// misleading — they would be overwritten on the first frame. Only the properties wgpu
// leaves alone are set below.

import Metal
import QuartzCore
import UIKit

/// A UIView backed by CAMetalLayer, plus the display link that drives the engine.
final class MetalCanvas: UIView {
    /// The process's MTLDevice, read back from the engine after `attachMetal`.
    ///
    /// Not used for drawing — the engine composites — but it is the handle MoltenVK will be
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

        // Everything else that used to be set here — device, pixelFormat, framebufferOnly,
        // colorspace, maximumDrawableCount — is set by the engine's `Surface::configure`.
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
        // targetTimestamp, not timestamp: the pacer wants when this frame will be *shown*,
        // not when the callback fired. FramePacer::plan() takes a timestamp for exactly this
        // reason — it never reads a clock of its own.
        let telemetry = engine.tick(nowMillis: link.targetTimestamp * 1000.0)
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
    }

    func willEnterForeground() {
        syncDrawableSize()
        engine.restoreGraphics()
    }

    func didBecomeActive() {
        start()
        engine.resume(nowMillis: CACurrentMediaTime() * 1000.0)
    }
}
