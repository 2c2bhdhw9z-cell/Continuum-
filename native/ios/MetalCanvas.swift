// Continuum — the one drawing surface, and the one MTLDevice.
//
// The architectural rule this file exists to enforce: **Swift creates exactly one
// MTLDevice and one MTLCommandQueue, and hands them to the engine.** Nothing else in the
// process calls MTLCreateSystemDefaultDevice(). An iPhone has exactly one GPU, and Metal
// resources belong to the MTLDevice that created them — so if wgpu, MoltenVK and ANGLE are
// each handed *this* device rather than making their own, every texture is shareable by
// construction and the core's frame reaches the compositor with no copy.
//
// See docs/SET_HW_RENDER_DESIGN.md §2 and §3.

import Metal
import QuartzCore
import UIKit

/// A UIView backed by CAMetalLayer, plus the display link that drives the engine.
final class MetalCanvas: UIView {
    // The single device and queue for the whole process.
    let device: MTLDevice
    let commandQueue: MTLCommandQueue

    private var displayLink: CADisplayLink?
    private let engine: ContinuumEngine
    private var attached = false

    /// Called once per presented frame with the engine's telemetry, for the HUD.
    var onTelemetry: ((TickTelemetry) -> Void)?

    override class var layerClass: AnyClass { CAMetalLayer.self }

    private var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    init(engine: ContinuumEngine) {
        guard let device = MTLCreateSystemDefaultDevice() else {
            fatalError("Metal is unavailable on this device")
        }
        guard let queue = device.makeCommandQueue() else {
            fatalError("could not create a Metal command queue")
        }
        self.device = device
        self.commandQueue = queue
        self.engine = engine
        super.init(frame: .zero)
        configureLayer()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    private func configureLayer() {
        metalLayer.device = device
        // BGRA8 to match what the cores render and what the layer wants natively.
        // Mismatching them costs a conversion pass on every frame.
        metalLayer.pixelFormat = .bgra8Unorm
        metalLayer.framebufferOnly = false

        // Two, not the default three. The third buffers throughput we do not need and
        // costs a frame of latency, which for an emulator is the wrong trade.
        metalLayer.maximumDrawableCount = 2

        // Pinned to sRGB rather than following the display. Emulated output predates
        // colour management entirely, and left alone the same ROM looks different on an
        // XDR panel than on an LCD — a saturated NES red rendered through P3 is not what
        // the hardware produced.
        metalLayer.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        metalLayer.wantsExtendedDynamicRangeContent = false

        // Presented as soon as the frame is ready rather than waiting for the next
        // Core Animation transaction.
        metalLayer.presentsWithTransaction = false
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
    /// Without `nativeScale` the image is soft on every device since the 6 Plus, because
    /// the layer would be rendered at point resolution and then upscaled by the compositor.
    private func syncDrawableSize() {
        let scale = window?.screen.nativeScale ?? UIScreen.main.nativeScale
        metalLayer.contentsScale = scale

        let size = CGSize(width: bounds.width * scale, height: bounds.height * scale)
        guard size.width > 0, size.height > 0 else { return }
        guard size != metalLayer.drawableSize else { return }
        metalLayer.drawableSize = size

        let width = UInt32(size.width)
        let height = UInt32(size.height)

        if !attached {
            attachEngine(width: width, height: height)
        } else {
            engine.resizeSurface(width: width, height: height)
        }
    }

    /// Hands the engine the three Metal handles it adopts.
    ///
    /// `Unmanaged.passUnretained` because these are owned here for the lifetime of the
    /// view, which outlives the engine's use of them. Passing retained would leak; passing
    /// a copy is not possible, since the whole point is that there is one device.
    private func attachEngine(width: UInt32, height: UInt32) {
        let devicePointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(device).toOpaque()))
        let queuePointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(commandQueue).toOpaque()))
        let layerPointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(metalLayer).toOpaque()))

        do {
            try engine.attachMetal(
                device: devicePointer,
                queue: queuePointer,
                layer: layerPointer,
                width: width,
                height: height
            )
            attached = true
        } catch {
            // Reported rather than fatal: the engine says which stage failed, and a
            // crash here would hide it behind a stack trace in Metal.
            NSLog("[continuum] attachMetal failed: \(error)")
        }
    }

    // MARK: - The frame loop

    /// One CADisplayLink, driving the engine. The peer of the web build's single
    /// requestAnimationFrame loop, and the same rule: there is only ever one.
    func start() {
        guard displayLink == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        // Let the system choose within a range rather than pinning 60: a ProMotion display
        // can run 120 and a thermally throttled device cannot hold 60, and the pacer
        // handles a variable interval already because it takes a timestamp.
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
        // not when the callback fired. FramePacer::plan() takes a timestamp for exactly
        // this reason — it never reads a clock of its own.
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
