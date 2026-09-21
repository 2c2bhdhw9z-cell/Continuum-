// Continuum - the audio output path, and the one place a sample crosses a thread.
//
// Until this file existed the engine had no output at all. The Rust ring filled up, the HUD
// reported a latency that only grew, and nothing ever read it. The reason that state of
// affairs lasted is that the obvious fix is the wrong one, so it is worth saying plainly what
// the shape here is and why it is not the short version.
//
// THE SHAPE: PUSH FROM THE TICK, PULL FROM THE RENDER BLOCK, LOCK-FREE RING IN BETWEEN.
//
//   CADisplayLink (main thread)                     audio IO thread (real-time)
//   ---------------------------                     ---------------------------
//   engine.applyGamepad(...)
//   engine.tick(nowMillis:)      <- takes the engine Mutex for the whole tick
//   engine.drainAudio(maxFrames:) <- takes it again, briefly, and copies PCM out
//   ring.append(samples)                            ring.pull(into: scratch, frames:)
//   ring.publish()                                  deinterleave into the AudioBufferList
//                                                   pad the remainder with silence
//
// The short version would have the render block call `engine.drainAudio` directly. That is a
// real-time thread taking the same `Mutex` that the display link holds for the entire duration
// of every tick, sixty times a second. The audio thread would block on the main thread, which
// is priority inversion: the symptom is not a stall, it is clicks, dropouts and stutter, and
// it gets worse exactly when the emulator is working hardest. So the render block never calls
// into Rust, never takes a lock, never allocates and never blocks. Everything it touches was
// allocated when this object was built.
//
// WHY THERE IS NO ATOMIC IN SIGHT, AND WHY THE RING IS STILL CORRECT.
//
// A single-producer single-consumer ring needs two things: indices that cannot be read
// half-written, and an ordering guarantee that the samples are visible before the index that
// claims they exist.
//
// The first is free. Each cursor is a naturally aligned machine word with exactly one writer,
// and this app ships arm64 only (project.yml pins ARCHS: arm64). There is no read-modify-write
// anywhere, so there is nothing for a compare-and-swap to do.
//
// The second is bought with arithmetic rather than with a fence, because Swift below iOS 18
// has no fence it can legally use: `Synchronization.Atomic` is iOS 18 and this app targets 16,
// C11 atomics are not importable, `OSMemoryBarrier` is deprecated and Apple's own position is
// that imported atomics are not to be relied on from Swift, and every lock is banned on a
// real-time thread by construction. So instead:
//
//   EACH SIDE PUBLISHES ITS CURSOR ONE CALL LATE.
//
// The producer stores the frontier it reached at the end of the PREVIOUS tick before appending
// anything new, so the consumer is only ever shown samples that were written at least one
// display frame ago, around 16 ms. The consumer does the same with its read cursor, so the
// producer is only ever shown a read position the consumer finished with in an earlier
// callback. In between, each thread executes a display-link dispatch or an audio IO cycle, a
// Rust mutex acquire and release, and a good deal of ARC traffic, every one of which is an
// atomic read-modify-write and therefore a full barrier on arm64. A store buffer drains in
// tens of cycles; this gives it millions.
//
// The residual error in both directions is benign by construction, which is the part that
// makes this design rather than a hope. Cursors only ever increase, so a stale read is always
// a SMALLER number. A consumer that reads a stale publish sees LESS audio available and
// underruns a hair early. A producer that reads a stale consume sees LESS free space and asks
// Rust for fewer samples, where Rust's own ring already has documented overrun behaviour.
// Neither can ever see a cursor further ahead than the truth, so neither can read a slot that
// has not been written or overwrite a slot that has not been read.
//
// When the deployment target reaches iOS 18 this becomes two `Atomic<Int>` with explicit
// `.releasing` and `.acquiring` orderings and the one-call lag can go. Until then the lag IS
// the ordering guarantee, so do not "simplify" it away.
//
// DIAGNOSTICS, BECAUSE NO SOUND AND NOT IMPLEMENTED LOOK IDENTICAL.
//
// This is a sideloaded build with no debugger, so every failure path below writes a distinct
// sentence into `status` and the HUD shows it. Nothing here fails silently and nothing reports
// a number it is guessing at: the sample rate is the one the session actually gave us, the
// latency is the sum of both rings divided by that rate, and "running" means the render block
// has really been called. The render block itself writes no strings, because composing one
// would allocate; it moves counters, and the read-out turns those into words on the main
// thread.

import AVFoundation
import Foundation

/// A single-producer, single-consumer ring of interleaved stereo float samples.
///
/// The producer is the display link, through `append` and `publish`. The consumer is the audio
/// render block, through `pull`. Nobody else may call either side. See the file header for why
/// this needs no locks and no atomics, and for the one rule that keeps it true: each side
/// publishes its cursor one call late.
///
/// Every cursor lives in one raw allocation rather than in stored properties. That is not
/// micro-optimisation: a `var` on a Swift class can carry dynamic exclusivity enforcement,
/// which is a runtime call, and a runtime call is exactly what a render block must not make.
/// Raw words have no language machinery attached to them at all.
final class AudioSampleRing {
    /// Interleaved stereo. One frame is two samples, always.
    static let channels = 2

    /// Where each cursor and counter lives in the shared word block.
    ///
    /// Named rather than numbered at the call sites, and grouped by who writes them, because
    /// "exactly one writer per word" is the whole safety argument and a second writer would be
    /// invisible in a bare index.
    private enum Slot {
        /// Written by the producer, read by the consumer. Samples the consumer may read.
        static let published = 0
        /// Written by the consumer, read by the producer. Samples the consumer is finished with.
        static let consumed = 1
        /// Producer only. Samples written, including those not yet published.
        static let writeFrontier = 2
        /// Consumer only. Samples read.
        static let readFrontier = 3
        /// Consumer only. Zero while the ring is refilling after a gap.
        static let primed = 4
        /// Set while the graph is stopped. How much to bank before playback starts.
        static let primeSamples = 5
        /// Written by the consumer, read by the main thread for the HUD.
        static let underruns = 6
        static let silenceFrames = 7
        static let renderedFrames = 8
        static let primeWaits = 9
        /// Written by the producer, read by the main thread for the HUD.
        static let dropped = 10
        static let oversizeRequests = 11
        static let unsupportedLayouts = 12
        static let count = 13
    }

    /// Sample storage, allocated once and never resized. A power of two, so the wrap is a mask
    /// rather than a division: the render block does this on every callback.
    private let storage: UnsafeMutablePointer<Float>
    private let capacity: Int
    private let mask: Int
    private let shared: UnsafeMutablePointer<Int>

    /// `frames` is rounded up to a power of two, in interleaved stereo frames.
    init(frames: Int) {
        var samples = max(1024, frames) * Self.channels
        // Round up to a power of two so `& mask` is exact.
        var rounded = 1024
        while rounded < samples { rounded <<= 1 }
        samples = rounded

        capacity = samples
        mask = samples - 1
        storage = UnsafeMutablePointer<Float>.allocate(capacity: samples)
        storage.initialize(repeating: 0, count: samples)
        shared = UnsafeMutablePointer<Int>.allocate(capacity: Slot.count)
        shared.initialize(repeating: 0, count: Slot.count)
    }

    deinit {
        storage.deinitialize(count: capacity)
        storage.deallocate()
        shared.deinitialize(count: Slot.count)
        shared.deallocate()
    }

    /// Interleaved stereo frames the ring can hold.
    var capacityFrames: Int { capacity / Self.channels }

    // MARK: - The producer side. Display link only.

    /// Makes everything written before now readable.
    ///
    /// MUST BE CALLED EVERY TICK, INCLUDING TICKS THAT APPEND NOTHING. This is the store that
    /// hands the previous tick's samples to the render block, so a tick that skips it strands
    /// them: the consumer would go silent while the ring quietly filled, which is precisely the
    /// bug this whole file exists to fix, only harder to see.
    func publish() {
        shared[Slot.published] = shared[Slot.writeFrontier]
    }

    /// Frames the producer may write without overtaking the consumer.
    ///
    /// Computed against the consumer's published position, so it under-reports by up to one
    /// render quantum and never over-reports. Under-reporting costs a few samples left in the
    /// Rust ring for one tick; over-reporting would corrupt audio the consumer is reading.
    var freeFrames: Int {
        let inFlight = shared[Slot.writeFrontier] - shared[Slot.consumed]
        return max(0, capacity - inFlight) / Self.channels
    }

    /// Frames written but not yet consumed, published or otherwise. The honest latency figure.
    var bufferedFrames: Int {
        max(0, shared[Slot.writeFrontier] - shared[Slot.consumed]) / Self.channels
    }

    /// Copies interleaved stereo samples in. They become readable on the next `publish`.
    ///
    /// A short copy is counted rather than blocked or wrapped: the caller already bounds its
    /// request by `freeFrames`, so this can only happen if the consumer stalled between the two
    /// calls, and dropping the newest is the right end to drop when the device is behind.
    func append(_ samples: UnsafeBufferPointer<Float>) {
        guard let base = samples.baseAddress, !samples.isEmpty else { return }

        let frontier = shared[Slot.writeFrontier]
        let free = capacity - (frontier - shared[Slot.consumed])
        let count = min(samples.count, max(0, free))
        if count < samples.count {
            shared[Slot.dropped] += (samples.count - count) / Self.channels
        }
        guard count > 0 else { return }

        // Two copies at most, rather than a per-sample modulo loop.
        let start = frontier & mask
        let first = min(capacity - start, count)
        storage.advanced(by: start).update(from: base, count: first)
        if count > first {
            storage.update(from: base.advanced(by: first), count: count - first)
        }
        shared[Slot.writeFrontier] = frontier + count
    }

    // MARK: - The consumer side. Render block only.

    /// How much audio to bank before the first sample is played, in frames.
    ///
    /// Set only while the graph is stopped, so the consumer cannot see it change underneath a
    /// callback.
    func setPrimeFrames(_ frames: Int) {
        shared[Slot.primeSamples] = max(0, frames) * Self.channels
    }

    /// Copies up to `frames` interleaved frames into `destination`, returning frames copied.
    ///
    /// Real-time safe: no allocation, no locking, no Rust, no Swift runtime calls. A short
    /// return is the caller's cue to pad with silence, which is the only correct thing to emit
    /// on a gap. Repeating the last samples would turn a 2 ms hole into a buzz, and leaving the
    /// buffer untouched would replay whatever CoreAudio had there.
    func pull(into destination: UnsafeMutablePointer<Float>, frames: Int) -> Int {
        // Publish first, for the reason in the file header: the producer is only ever shown a
        // read position this block finished with in an earlier callback.
        shared[Slot.consumed] = shared[Slot.readFrontier]

        let wanted = frames * Self.channels
        guard wanted > 0 else { return 0 }

        let frontier = shared[Slot.readFrontier]
        let available = max(0, shared[Slot.published] - frontier)

        // Refilling. Playing out a nearly empty ring one sample at a time produces a
        // continuous crackle; waiting for a known depth and then running cleanly produces one
        // gap. The gap is the better artefact, and it is also the one that recovers.
        if shared[Slot.primed] == 0 {
            if available < shared[Slot.primeSamples] {
                shared[Slot.primeWaits] += 1
                return 0
            }
            shared[Slot.primed] = 1
        }

        let count = min(wanted, available)
        if count > 0 {
            let start = frontier & mask
            let first = min(capacity - start, count)
            destination.update(from: storage.advanced(by: start), count: first)
            if count > first {
                destination.advanced(by: first).update(from: storage, count: count - first)
            }
            shared[Slot.readFrontier] = frontier + count
            shared[Slot.renderedFrames] += count / Self.channels
        }
        if count < wanted {
            shared[Slot.underruns] += 1
            shared[Slot.silenceFrames] += (wanted - count) / Self.channels
            // Re-prime rather than limp along at whatever depth starved us.
            shared[Slot.primed] = 0
        }
        return count / Self.channels
    }

    /// The render block asked for more frames than the scratch buffer holds.
    ///
    /// Counted rather than handled, because the answer is to enlarge the scratch buffer, and
    /// that is a decision for whoever reads the HUD rather than something to do at render time.
    func countOversizeRequest() {
        shared[Slot.oversizeRequests] += 1
    }

    /// The buffer list had a channel layout this code cannot describe, so silence went out.
    func countUnsupportedLayout() {
        shared[Slot.unsupportedLayouts] += 1
    }

    // MARK: - The read-out. Main thread only, and stale by design.

    /// Frames the render block has actually played. Zero here while `isRunning` is true is the
    /// single most useful fact on the HUD: it means the graph came up and the render block is
    /// not being called.
    var renderedFrames: Int { shared[Slot.renderedFrames] }
    var underruns: Int { shared[Slot.underruns] }
    var silenceFrames: Int { shared[Slot.silenceFrames] }
    var primeWaits: Int { shared[Slot.primeWaits] }
    var droppedFrames: Int { shared[Slot.dropped] }
    var oversizeRequests: Int { shared[Slot.oversizeRequests] }
    var unsupportedLayouts: Int { shared[Slot.unsupportedLayouts] }

    /// Empties the ring. Only legal while the graph is stopped, because it moves both cursors.
    func reset() {
        shared.update(repeating: 0, count: Slot.count)
        storage.update(repeating: 0, count: capacity)
    }
}

/// The device half of the audio path: the session, the graph, and the lifecycle around them.
///
/// Owned by `EngineHost` for the app's lifetime, for the same reason `padInput` is: SwiftUI
/// rebuilds views freely and the display link must never be left holding an object that was
/// replaced. Built with the SAME `ContinuumEngine` the canvas ticks, because the whole design
/// rests on the drain happening on the display link's thread and nowhere else.
final class AudioOutput {
    /// Interleaved stereo frames the Swift ring holds.
    ///
    /// 8192 frames is 170 ms at 48 kHz and 128 KB of storage, allocated once. Sized generously
    /// on purpose: it is not the latency, it is the headroom. Steady-state depth is set by
    /// `primeSeconds` below, and the spare capacity is what absorbs a tick that ran late
    /// without the ring having to drop anything. Rate independent, so a route change never
    /// reallocates.
    private static let ringFrames = 8192

    /// The largest render request this can serve from its scratch buffer.
    ///
    /// iOS asks for 512 frames or fewer in practice. 4096 is eight times that, so an oversized
    /// request is a genuine surprise worth counting rather than a routine case worth handling.
    static let maxRenderFrames = 4096

    /// Matches the Rust ceiling in `uniffi_api.rs`, and is bounded there too. Restated rather
    /// than guessed: asking for more is harmless, it is just never useful.
    private static let maxDrainFrames = 4096

    /// How much audio to bank before the first sample plays, in seconds.
    ///
    /// Two video frames at 60 fps. Enough that one late tick does not produce a gap, little
    /// enough that a button press and its sound still feel simultaneous. Expressed as a time so
    /// it means the same thing at 44100 and at 48000.
    private static let primeSeconds = 0.033

    let ring: AudioSampleRing

    /// Deinterleave staging, allocated once. The ring is interleaved because that is what
    /// libretro cores produce and what the Rust ring stores; AVAudioEngine's float format is
    /// not. Pulling into this and scattering out of it costs one extra copy of a few kilobytes
    /// and keeps the ring free of any CoreAudio shape.
    private let scratch: UnsafeMutablePointer<Float>

    private let engine: ContinuumEngine
    private var audioEngine: AVAudioEngine?
    private var sourceNode: AVAudioSourceNode?
    private var observers: [NSObjectProtocol] = []

    /// The sentence the HUD shows. Never empty, and never a guess.
    private(set) var status = "audio: not started"

    /// Whether the graph is up. Not the same as audible: see `ring.renderedFrames`.
    private(set) var isRunning = false

    /// The rate the ring is filled and drained at, read from the session rather than assumed.
    private(set) var sampleRate: Double = 0

    /// What the output hardware said it runs at, when it disagreed with the session.
    private(set) var hardwareRate: Double = 0

    /// Whether a caller wants audio at all. `suspend` and `resume` only act when this is set,
    /// so backgrounding while the Library is on screen cannot start a graph nobody asked for.
    private var wanted = false

    private var interrupted = false
    private(set) var rebuilds = 0

    init(engine: ContinuumEngine) {
        self.engine = engine
        ring = AudioSampleRing(frames: Self.ringFrames)
        scratch = UnsafeMutablePointer<Float>.allocate(
            capacity: Self.maxRenderFrames * AudioSampleRing.channels
        )
        scratch.initialize(repeating: 0, count: Self.maxRenderFrames * AudioSampleRing.channels)
        // Registered once, here, rather than at start. An interruption that arrives between two
        // start attempts still has to be seen, and a notification observer added on the way up
        // is an observer that is missing on exactly the paths that go wrong.
        observeSession()
    }

    deinit {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        scratch.deinitialize(count: Self.maxRenderFrames * AudioSampleRing.channels)
        scratch.deallocate()
    }

    // MARK: - Starting and stopping

    /// Brings the session and the graph up. Called when a game launches.
    ///
    /// Idempotent: a second launch while audio is already running rebuilds nothing, because the
    /// rate cannot have changed without a route change, and a route change has its own path.
    func start() {
        wanted = true
        guard !isRunning else {
            status = "audio: already running at \(rateText) Hz"
            return
        }
        interrupted = false
        ring.reset()
        _ = startGraph(reason: "start")
    }

    /// Tears the graph down and releases the session. Called when the session stops.
    func stop() {
        wanted = false
        interrupted = false
        teardownGraph()
        // Status set BEFORE the deactivation, because `deactivateSession` appends its own failure
        // to whatever is there and would otherwise be appending to the previous line.
        status = "audio: stopped with the session"
        deactivateSession()
    }

    /// Backgrounding. The display link has stopped, so nothing would feed the ring and the
    /// render block would emit an unbroken run of silence while counting underruns.
    func suspend() {
        guard wanted, isRunning else { return }
        audioEngine?.pause()
        isRunning = false
        status = "audio: suspended for the background"
        deactivateSession()
    }

    /// Foregrounding. Rebuilt rather than merely unpaused, because the route may well have
    /// changed while the app was away and the rate with it.
    ///
    /// `startGraph` writes the status either way, so nothing is overwritten here: its success
    /// line already names the reason and any hardware rate disagreement, and its failure line
    /// names the stage that failed.
    func resume() {
        guard wanted, !isRunning else { return }
        if interrupted {
            status = "audio: still interrupted, so playback was not resumed on foregrounding"
            return
        }
        rebuild(reason: "on foregrounding")
    }

    // MARK: - The per-tick push

    /// Moves one tick's worth of audio from the engine's ring into the device ring.
    ///
    /// CALLED FROM THE DISPLAY LINK, IMMEDIATELY AFTER `tick`, AND FROM NOWHERE ELSE. After
    /// rather than before, because the samples this tick's core steps just produced are the
    /// ones worth having; draining first would always be one frame stale.
    ///
    /// The publish happens unconditionally, before anything else and regardless of whether
    /// there is anything new to append. It is what hands the PREVIOUS tick's samples to the
    /// render block, so a path that returns early without it would leave the consumer silent
    /// while the ring filled behind it.
    func pump() {
        guard isRunning else { return }
        ring.publish()

        let room = min(ring.freeFrames, Self.maxDrainFrames)
        guard room > 0 else { return }

        // One allocation per tick, on the display link's thread, and the only copy in the whole
        // path. UniFFI has no way to write into a caller's buffer, so this is the cost of the
        // boundary; it is about 6 KB at 48 kHz and it buys a render block that never touches
        // the engine lock. See `ContinuumEngine::drain_audio`.
        let samples = engine.drainAudio(maxFrames: UInt32(room))
        guard !samples.isEmpty else { return }
        samples.withUnsafeBufferPointer { ring.append($0) }
    }

    // MARK: - Building the graph

    /// Configures the session, builds the graph and starts it.
    ///
    /// Returns false having written a specific reason into `status`. Every failure here is a
    /// different sentence, because on a sideloaded build the HUD is the only debugger and
    /// "no sound" otherwise reads exactly like "not implemented".
    private func startGraph(reason: String) -> Bool {
        let session = AVAudioSession.sharedInstance()

        // `.playback` rather than `.ambient` or `.soloAmbient`, because a game with its sound
        // switched off by the ringer switch reads as the feature being broken. `.playback` also
        // survives the screen locking, which matters for a long RPG session.
        do {
            try session.setCategory(.playback, mode: .default, options: [])
        } catch {
            isRunning = false
            status = "audio: the session would not take the playback category: \(error)"
            return false
        }

        do {
            try session.setActive(true)
        } catch {
            isRunning = false
            status = "audio: the session would not activate: \(error)"
            return false
        }

        // THE RATE COMES FROM THE SESSION, NEVER FROM A CONSTANT. A modern iPhone speaker is
        // 48000, several Bluetooth routes are 44100, and a wired interface can be neither.
        // `EmulatorBridge` defaults to 48000 because something has to be assumed before the
        // graph exists, and that assumption is what made the old HUD line a guess.
        var rate = session.sampleRate
        // Carried rather than written straight into `status`, because the success line below
        // replaces `status` wholesale and a warning written here would be lost exactly when it
        // mattered most.
        var note = ""
        if !(rate.isFinite && rate >= 8000 && rate <= 192_000) {
            note = " (the session reported an unusable rate of \(rate) Hz, so 48000 was used)"
            rate = 48_000
        }

        guard let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: rate,
            channels: 2,
            interleaved: false
        ) else {
            isRunning = false
            status = "audio: could not build a 2 channel float format at \(Int(rate)) Hz"
            return false
        }

        sampleRate = rate
        // Resampling happens in Rust, where `audio/resample.rs` already does it and is already
        // tested. Told before the first render so the very first samples the device sees are at
        // its own rate rather than at the core's.
        engine.setOutputSampleRate(rate: UInt32(rate.rounded()))
        ring.setPrimeFrames(Int(rate * Self.primeSeconds))

        let node = makeSourceNode(format: format)
        let graph = AVAudioEngine()
        graph.attach(node)
        // Through the main mixer rather than straight to the output node, so the format
        // conversion the engine may need has somewhere to live and a future volume control has
        // a node to sit on.
        graph.connect(node, to: graph.mainMixerNode, format: format)
        graph.prepare()

        do {
            try graph.start()
        } catch {
            isRunning = false
            sourceNode = nil
            audioEngine = nil
            status = "audio: the engine would not start at \(Int(rate)) Hz: \(error)"
            return false
        }

        audioEngine = graph
        sourceNode = node
        isRunning = true

        // A disagreement here is not fatal: AVAudioEngine inserts a converter and the sound
        // still comes out. It is worth saying out loud because it means the device is
        // resampling on top of the resampling Rust already did, which is a quality and latency
        // cost somebody should know about rather than discover.
        let output = graph.outputNode.outputFormat(forBus: 0).sampleRate
        hardwareRate = output
        if output > 0, abs(output - rate) > 1 {
            status = "audio: running at \(Int(rate)) Hz but the output hardware is at "
                + "\(Int(output)) Hz, so the engine is converting (\(reason))" + note
        } else {
            status = "audio: running at \(Int(rate)) Hz (\(reason))" + note
        }
        return true
    }

    /// The render block. Everything it needs is captured by value or is a `final class`
    /// reference resolved at build time, so calling it costs no ARC traffic and no dispatch.
    ///
    /// `self` is deliberately NOT captured. The block runs on the audio IO thread and must not
    /// keep this object alive or reach anything on it that the main thread might be writing.
    private func makeSourceNode(format: AVAudioFormat) -> AVAudioSourceNode {
        let ring = self.ring
        let scratch = self.scratch
        let maxFrames = Self.maxRenderFrames

        return AVAudioSourceNode(format: format) { isSilence, _, frameCount, audioBufferList in
            let frames = Int(frameCount)
            let buffers = UnsafeMutableAudioBufferListPointer(audioBufferList)

            // Bounded against the scratch buffer rather than trusted. A request larger than
            // this cannot be served, so the excess becomes silence and the surprise is counted.
            var usable = frames
            if usable > maxFrames {
                ring.countOversizeRequest()
                usable = maxFrames
            }

            // Pulled once, before the buffer geometry is known, because the ring must be read on
            // exactly one code path no matter which layout comes back. If a buffer turns out to
            // hold fewer frames than it asked for, the excess is discarded rather than pushed
            // back: the ring is single-reader by construction and giving it a rewind would mean
            // a second writer for the read cursor, which is the one thing that would make it
            // unsafe. CoreAudio sizes these buffers from the same frame count it passes in, so
            // this is a bound rather than a case that happens.
            let filled = usable > 0 ? ring.pull(into: scratch, frames: usable) : 0

            let count = buffers.count
            if count == 0 {
                isSilence.pointee = ObjCBool(true)
                return noErr
            }

            if count == 1 {
                // One buffer means either interleaved stereo or mono. Both are handled, because
                // the format this node was built with is a request and not a guarantee, and a
                // wrong assumption here is silence with nothing to read.
                guard let raw = buffers[0].mData else {
                    isSilence.pointee = ObjCBool(true)
                    return noErr
                }
                let out = raw.assumingMemoryBound(to: Float.self)
                let channels = Int(buffers[0].mNumberChannels)
                let room = Int(buffers[0].mDataByteSize)
                    / (MemoryLayout<Float>.size * max(1, channels))
                let span = min(frames, room)

                if channels == AudioSampleRing.channels {
                    let copied = min(filled, span) * AudioSampleRing.channels
                    if copied > 0 { out.update(from: scratch, count: copied) }
                    let total = span * AudioSampleRing.channels
                    if total > copied {
                        out.advanced(by: copied).update(repeating: 0, count: total - copied)
                    }
                } else if channels == 1 {
                    // Averaged rather than left-only, so a mono route does not silently throw
                    // away half the mix.
                    let copied = min(filled, span)
                    var frame = 0
                    while frame < copied {
                        out[frame] = 0.5 * (scratch[frame * 2] + scratch[frame * 2 + 1])
                        frame += 1
                    }
                    if span > copied {
                        out.advanced(by: copied).update(repeating: 0, count: span - copied)
                    }
                } else {
                    // Some layout this code cannot describe. Silence, counted, rather than
                    // stereo smeared across channels it does not belong in.
                    ring.countUnsupportedLayout()
                    out.update(repeating: 0, count: span * channels)
                    isSilence.pointee = ObjCBool(true)
                    return noErr
                }

                if filled == 0 { isSilence.pointee = ObjCBool(true) }
                return noErr
            }

            // Two or more buffers means deinterleaved, one channel each, which is what
            // AVAudioEngine's float format asks for.
            let left = buffers[0].mData?.assumingMemoryBound(to: Float.self)
            let right = buffers[1].mData?.assumingMemoryBound(to: Float.self)
            let leftRoom = Int(buffers[0].mDataByteSize) / MemoryLayout<Float>.size
            let rightRoom = Int(buffers[1].mDataByteSize) / MemoryLayout<Float>.size

            if let left {
                let span = min(frames, leftRoom)
                let copied = min(filled, span)
                var frame = 0
                while frame < copied {
                    left[frame] = scratch[frame * 2]
                    frame += 1
                }
                if span > copied {
                    left.advanced(by: copied).update(repeating: 0, count: span - copied)
                }
            }
            if let right {
                let span = min(frames, rightRoom)
                let copied = min(filled, span)
                var frame = 0
                while frame < copied {
                    right[frame] = scratch[frame * 2 + 1]
                    frame += 1
                }
                if span > copied {
                    right.advanced(by: copied).update(repeating: 0, count: span - copied)
                }
            }
            // Anything past the second channel is silence rather than whatever CoreAudio left
            // in the buffer.
            if count > 2 {
                var index = 2
                while index < count {
                    if let extra = buffers[index].mData?.assumingMemoryBound(to: Float.self) {
                        let room = Int(buffers[index].mDataByteSize) / MemoryLayout<Float>.size
                        extra.update(repeating: 0, count: min(frames, room))
                    }
                    index += 1
                }
            }

            if filled == 0 { isSilence.pointee = ObjCBool(true) }
            return noErr
        }
    }

    private func teardownGraph() {
        if let node = sourceNode, let graph = audioEngine {
            graph.stop()
            graph.disconnectNodeOutput(node)
            graph.detach(node)
        } else {
            audioEngine?.stop()
        }
        sourceNode = nil
        audioEngine = nil
        isRunning = false
    }

    private func deactivateSession() {
        // Released so another app can have the audio route back. A failure here is worth
        // recording but is not worth overwriting a more useful status with, so it only speaks
        // up when nothing more important is on the line.
        do {
            try AVAudioSession.sharedInstance().setActive(false)
        } catch {
            if status.hasPrefix("audio: stopped") || status.hasPrefix("audio: suspended") {
                status += " (the session would not deactivate: \(error))"
            }
        }
    }

    /// Throws the graph away and builds a new one. For a route or configuration change, where
    /// the rate the node was built with may no longer be the rate the device runs at.
    private func rebuild(reason: String) {
        guard wanted else { return }
        rebuilds += 1
        teardownGraph()
        // Both rings, together. The queued tail was resampled for a device that has gone, and
        // leaving one end full while the other is empty would make the reported latency a lie.
        ring.reset()
        engine.flushAudio()
        _ = startGraph(reason: reason)
    }

    // MARK: - Interruptions and route changes

    private func observeSession() {
        let centre = NotificationCenter.default
        observers = [
            centre.addObserver(
                forName: AVAudioSession.interruptionNotification,
                object: AVAudioSession.sharedInstance(),
                queue: .main
            ) { [weak self] note in
                self?.handleInterruption(note)
            },
            centre.addObserver(
                forName: AVAudioSession.routeChangeNotification,
                object: AVAudioSession.sharedInstance(),
                queue: .main
            ) { [weak self] note in
                self?.handleRouteChange(note)
            },
            // Fired when the engine's own IO format changes underneath it, which a route change
            // can do. Without handling it the graph keeps running against a format that no
            // longer exists and goes quietly silent.
            centre.addObserver(
                forName: NSNotification.Name.AVAudioEngineConfigurationChange,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                self?.handleConfigurationChange()
            },
        ]
    }

    /// A phone call, a timer, Siri. The system takes the session away and gives it back.
    private func handleInterruption(_ note: Notification) {
        guard let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: raw) else {
            status = "audio: an interruption arrived with no type, so nothing was changed"
            return
        }

        switch type {
        case .began:
            interrupted = true
            // The session is already inactive by the time this arrives. Stopping the graph is
            // about not leaving a render block spinning against a dead route.
            audioEngine?.pause()
            isRunning = false
            status = "audio: interrupted by the system"
        case .ended:
            interrupted = false
            guard wanted else {
                status = "audio: the interruption ended while no game was running"
                return
            }
            let optionsRaw = note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
            let options = AVAudioSession.InterruptionOptions(rawValue: optionsRaw)
            // Rebuilt rather than unpaused. An interruption can change the route, so the rate
            // the node was built with is no longer something to take on trust.
            rebuild(reason: "after the interruption")
            if !options.contains(.shouldResume), isRunning {
                status += " (the system did not ask for a resume, so it was restarted anyway)"
            }
        @unknown default:
            status = "audio: an unrecognised interruption type \(raw) arrived"
        }
    }

    /// Headphones unplugged, a Bluetooth device gone, a new route arrived.
    ///
    /// The unplug case is the one that has to work: iOS pauses playback when the old device goes
    /// away, and an app that does nothing here is permanently silent afterwards with no error to
    /// read anywhere.
    private func handleRouteChange(_ note: Notification) {
        guard wanted else { return }
        guard let raw = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
              let reason = AVAudioSession.RouteChangeReason(rawValue: raw) else {
            // A route change with no readable reason still changed the route, and the safe answer
            // is the same as for the ones we do understand.
            rebuild(reason: "after an unreadable route change")
            return
        }
        let name = Self.describe(reason)

        switch reason {
        case .oldDeviceUnavailable, .newDeviceAvailable, .override, .categoryChange,
             .routeConfigurationChange, .wakeFromSleep, .noSuitableRouteForCategory:
            // Every one of these can change the sample rate, and an `AVAudioSourceNode`'s format
            // is fixed when it is constructed, so the graph is rebuilt rather than poked. It costs
            // one refill of the prime buffer, which is 33 ms nobody notices during an unplug.
            rebuild(reason: "after the route changed (\(name))")
        default:
            if isRunning {
                status = "audio: the route changed (\(name)) and playback continued at "
                    + "\(rateText) Hz"
            } else {
                rebuild(reason: "after the route changed (\(name))")
            }
        }
    }

    /// The engine's own IO format changed underneath it, which a route change can do without
    /// going through the session. Ignoring this leaves a running graph attached to a format that
    /// no longer exists, and it goes quietly silent.
    private func handleConfigurationChange() {
        guard wanted else { return }
        rebuild(reason: "after the engine configuration changed")
    }

    private static func describe(_ reason: AVAudioSession.RouteChangeReason) -> String {
        switch reason {
        case .newDeviceAvailable: return "a device was plugged in"
        case .oldDeviceUnavailable: return "a device was unplugged"
        case .categoryChange: return "the category changed"
        case .override: return "the route was overridden"
        case .wakeFromSleep: return "the device woke"
        case .noSuitableRouteForCategory: return "no route suits playback"
        case .routeConfigurationChange: return "the route was reconfigured"
        case .unknown: return "reason unknown"
        @unknown default: return "reason \(reason.rawValue)"
        }
    }

    // MARK: - The read-out

    /// The rate as the HUD should print it, without inventing one when there is none.
    var rateText: String {
        sampleRate > 0 ? String(Int(sampleRate)) : "no"
    }

    /// True once the render block has provably run. `isRunning` says the graph came up;
    /// this says sound is actually being asked for, which is a different question and the one
    /// that separates a broken graph from a broken push.
    var hasRendered: Bool { ring.renderedFrames > 0 }
}
