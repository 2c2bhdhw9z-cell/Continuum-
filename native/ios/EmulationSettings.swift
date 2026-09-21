// How the game looks, how fast it runs, how loud it is, and how far back it can be wound.
//
// These five preferences share one awkward property that shaped this whole file: the engine
// objects that consume them do not outlive a game. The frame pacer is rebuilt by every launch
// and every stop, and the renderer's frame target is released when a session ends. So a
// setting is not a thing you write once into the engine and forget; it is a thing you own out
// here and re-assert. The engine now remembers them too, on its own side, which means the two
// halves agree and neither has to trust the other's ordering.
//
// THE RULE THE SETTINGS SCREEN IS STILL BUILT AROUND: every control does something today.
// That rule is why this file exists rather than a longer apology in SettingsScreen. The
// controls the design reference always showed for fit, filter, speed and volume are real now
// because the engine exports them, and rewind is real because the engine grew a tape. What is
// still missing is still listed as missing, and is now a much shorter list.
//
// One honest limit is encoded here rather than hidden. The engine refuses to run more than
// four emulated frames per display tick, so on a 60 Hz screen fast-forward tops out near 4x
// whatever multiplier is requested, and asking for more is answered with dropped frames
// instead of speed. `FastForward.allCases` therefore stops at 4x. Offering 8x would be
// offering something the engine cannot deliver, which is the same sin as a dead switch.

import Foundation

/// The emulation preferences, persisted and applied to the engine.
///
/// A separate observable object from `EngineHost` rather than more properties on it, following
/// `ArtworkStore`: these have their own storage keys, their own defaults and their own reason
/// to exist, and `EngineHost` is already the largest type in the app.
@MainActor
final class EmulationSettings: ObservableObject {

    // MARK: The choices

    /// How the picture is fitted to the screen.
    enum ScreenFit: String, CaseIterable, Identifiable {
        case fit
        case integer
        case stretch

        var id: String { rawValue }

        var label: String {
            switch self {
            case .fit: return "Fit"
            case .integer: return "Pixel perfect"
            case .stretch: return "Fill"
            }
        }

        // There used to be a per-option `explanation` here, and it was removed rather than left
        // unused. Showing it under the selector meant the text changed length as the selection
        // changed, which changed the height of the section and slid the selector out from under
        // the finger still choosing on it. The wording now lives in one static note in
        // `SettingsScreen.pictureSection` that describes every option at once. See
        // `SegmentedChoice` for the whole account.

        var engineValue: ScaleModeOption {
            switch self {
            case .fit: return .aspectFit
            case .integer: return .integerScale
            case .stretch: return .stretch
            }
        }
    }

    /// How pixels are sampled when the picture is scaled up.
    enum PixelFilter: String, CaseIterable, Identifiable {
        case sharp
        case smooth

        var id: String { rawValue }

        var label: String {
            switch self {
            case .sharp: return "Sharp"
            case .smooth: return "Smooth"
            }
        }

        // Per-option `explanation` removed for the reason given on `ScreenFit` above.

        var engineValue: ScaleFilterOption {
            switch self {
            case .sharp: return .nearest
            case .smooth: return .linear
            }
        }
    }

    /// The multiplier used while the fast-forward button is held.
    ///
    /// Stops at 4x on purpose. See the note at the top of this file: beyond roughly 4x the
    /// engine drops the surplus rather than running faster, so a 8x entry here would be a
    /// control that reads 8x and delivers 4x.
    enum FastForward: String, CaseIterable, Identifiable {
        case oneAndAHalf
        case double
        case triple
        case quadruple

        var id: String { rawValue }

        var multiplier: Double {
            switch self {
            case .oneAndAHalf: return 1.5
            case .double: return 2.0
            case .triple: return 3.0
            case .quadruple: return 4.0
            }
        }

        var label: String {
            switch self {
            case .oneAndAHalf: return "1.5x"
            case .double: return "2x"
            case .triple: return "3x"
            case .quadruple: return "4x"
            }
        }
    }

    /// How much memory rewind may use. `off` disables it and frees the tape.
    ///
    /// Presented in megabytes because that is the promise the engine can actually keep. How
    /// many *seconds* it buys depends entirely on the running core: a PlayStation save state
    /// can be a hundred times the size of an NES one, so the same 64 MB is minutes on one
    /// system and seconds on another. `rewindReadout` says what it turned out to be worth on
    /// the game in front of you, which is the honest way round.
    /// Off, or on with the budget chosen for you.
    ///
    /// THIS USED TO OFFER 32, 96 AND 256 MB AND THE CHOICE WAS REMOVED ON PURPOSE. Asking somebody
    /// how many megabytes of rewind they want is asking them to do arithmetic they have no way of
    /// doing: how much time a budget buys depends on the running core's save-state size, which
    /// differs by a factor of about a hundred between the NES and the PlayStation, is not knowable
    /// before a game is launched, and is not a number anyone should have to think about. Three
    /// numbers that all mean "rewind, please" is a menu, not a setting.
    ///
    /// So "on" now sizes itself from the device, and `automaticBytes` explains how. The read-out
    /// still says what it worked out to and what it bought, which is the part that was always the
    /// useful half.
    ///
    /// The raw values of the two remaining cases are deliberately "off" and "on", and an older
    /// stored value of "small", "medium" or "large" is read as "on" rather than being discarded.
    /// See the restore in `init`.
    enum RewindBudget: String, CaseIterable, Identifiable {
        case off
        case on

        var id: String { rawValue }

        var label: String {
            switch self {
            case .off: return "Off"
            case .on: return "On"
            }
        }

        var bytes: UInt64 {
            switch self {
            case .off: return 0
            case .on: return EmulationSettings.automaticBytes
            }
        }
    }

    /// The rewind budget when rewind is on, chosen from how much memory the device has.
    ///
    /// **A share of physical memory rather than a fixed ceiling, because the risk being managed is
    /// not "wasting memory", it is iOS terminating the app.** A phone does not page an app out when
    /// it grows; it kills it. A fixed 256 MB is comfortable on a recent Pro and a much larger share
    /// of a 3 GB device, where the app is also holding a core's working memory, the audio rings,
    /// the Metal surface and a library of cover art. Scaling means the generous case stays generous
    /// without the small case being the one that gets killed.
    ///
    /// Four percent, clamped. The clamp is what makes it a promise rather than a formula: the floor
    /// keeps rewind worth switching on at all on a small device, and the ceiling stops a future
    /// phone with a great deal of memory from quietly reserving hundreds of megabytes for a feature
    /// the user may not be using this session.
    ///
    /// `physicalMemory` and not the memory free right now, which would be the more precise signal
    /// and the wrong one: it changes minute to minute, so the same setting would buy a different
    /// amount of rewind on each launch, and the read-out would be telling the truth about something
    /// that keeps moving. A figure derived from the hardware is stable and explainable.
    static var automaticBytes: UInt64 {
        let physical = ProcessInfo.processInfo.physicalMemory
        let share = physical / 25
        let floor: UInt64 = 48 * 1_048_576
        let ceiling: UInt64 = 384 * 1_048_576
        return min(max(share, floor), ceiling)
    }

    // MARK: Stored preferences

    @Published var screenFit: ScreenFit = .fit {
        didSet {
            guard oldValue != screenFit else { return }
            UserDefaults.standard.set(screenFit.rawValue, forKey: Self.fitKey)
            engine.setScaleMode(mode: screenFit.engineValue)
        }
    }

    @Published var pixelFilter: PixelFilter = .sharp {
        didSet {
            guard oldValue != pixelFilter else { return }
            UserDefaults.standard.set(pixelFilter.rawValue, forKey: Self.filterKey)
            engine.setFilter(filter: pixelFilter.engineValue)
        }
    }

    @Published var fastForward: FastForward = .double {
        didSet {
            guard oldValue != fastForward else { return }
            UserDefaults.standard.set(fastForward.rawValue, forKey: Self.fastForwardKey)
            // Takes effect immediately if the button happens to be held right now, which is
            // the only case where this matters and costs one call to handle.
            if isFastForwarding {
                engine.setSpeed(speed: fastForward.multiplier)
            }
        }
    }

    /// `0.0` to `1.0`.
    @Published var volume: Double = 1.0 {
        didSet {
            guard oldValue != volume else { return }
            UserDefaults.standard.set(volume, forKey: Self.volumeKey)
            engine.setVolume(volume: Float(volume))
        }
    }

    @Published var muted: Bool = false {
        didSet {
            guard oldValue != muted else { return }
            UserDefaults.standard.set(muted, forKey: Self.mutedKey)
            engine.setMuted(muted: muted)
        }
    }

    @Published var rewindBudget: RewindBudget = .off {
        didSet {
            guard oldValue != rewindBudget else { return }
            UserDefaults.standard.set(rewindBudget.rawValue, forKey: Self.rewindKey)
            engine.setRewindBudgetBytes(budgetBytes: rewindBudget.bytes)
        }
    }

    // MARK: Live state

    /// True while the fast-forward button is held. Drives the button's own appearance.
    @Published private(set) var isFastForwarding = false

    /// True while the rewind button is held.
    @Published private(set) var isRewinding = false

    /// What the rewind tape currently holds, refreshed from the engine for the Settings
    /// screen. Not read every frame: it is a paragraph of text, not telemetry.
    @Published private(set) var rewindReadout = "off"

    private let engine: ContinuumEngine

    private static let fitKey = "continuum.video.fit.v1"
    private static let filterKey = "continuum.video.filter.v1"
    private static let fastForwardKey = "continuum.speed.fastForward.v1"
    private static let volumeKey = "continuum.audio.volume.v1"
    private static let mutedKey = "continuum.audio.muted.v1"
    private static let rewindKey = "continuum.rewind.budget.v1"

    init(engine: ContinuumEngine) {
        self.engine = engine

        // Restored before anything can display, each falling back to its default rather than
        // to nil, so a first launch and a value corrupted by a crash behave the same way.
        // Note `didSet` does not fire for assignments made inside `init`, which is why
        // `applyAll` below is not redundant.
        let defaults = UserDefaults.standard
        if let raw = defaults.string(forKey: Self.fitKey), let stored = ScreenFit(rawValue: raw) {
            screenFit = stored
        }
        if let raw = defaults.string(forKey: Self.filterKey),
           let stored = PixelFilter(rawValue: raw) {
            pixelFilter = stored
        }
        if let raw = defaults.string(forKey: Self.fastForwardKey),
           let stored = FastForward(rawValue: raw) {
            fastForward = stored
        }
        if let stored = defaults.object(forKey: Self.volumeKey) as? Double {
            volume = stored.clamped(to: 0.0...1.0)
        }
        if let stored = defaults.object(forKey: Self.mutedKey) as? Bool {
            muted = stored
        }
        if let raw = defaults.string(forKey: Self.rewindKey) {
            // An older build stored "small", "medium" or "large" here. Those all meant rewind was
            // wanted, so they are read as "on" rather than falling back to the default, which would
            // silently switch the feature off for anyone who had already turned it on.
            rewindBudget = RewindBudget(rawValue: raw) ?? (raw == "off" ? .off : .on)
        }

        applyAll()
    }

    /// Pushes every preference into the engine.
    ///
    /// Called once from `init`, and available for any later moment where the engine's state
    /// might not reflect these values. The engine holds them across launches itself, so this
    /// is not needed per game, but it is cheap and idempotent and makes the intent explicit.
    func applyAll() {
        engine.setScaleMode(mode: screenFit.engineValue)
        engine.setFilter(filter: pixelFilter.engineValue)
        engine.setVolume(volume: Float(volume))
        engine.setMuted(muted: muted)
        engine.setRewindBudgetBytes(budgetBytes: rewindBudget.bytes)
        // Deliberately not setting speed: 1x is the resting state and fast-forward is a held
        // button, so pushing a multiplier here would start every game fast.
        engine.setSpeed(speed: 1.0)
    }

    // MARK: Fast forward

    func beginFastForward() {
        guard !isFastForwarding else { return }
        isFastForwarding = true
        engine.setSpeed(speed: fastForward.multiplier)
    }

    func endFastForward() {
        guard isFastForwarding else { return }
        isFastForwarding = false
        engine.setSpeed(speed: 1.0)
    }

    // MARK: Rewind

    var rewindEnabled: Bool { rewindBudget != .off }

    /// Starts winding backwards. The engine's own tick does the work from here, so there is
    /// nothing to call per frame; see `ContinuumEngine.setRewinding`.
    func beginRewind() {
        guard rewindEnabled, !isRewinding else { return }
        isRewinding = true
        engine.setRewinding(rewinding: true)
    }

    func endRewind() {
        guard isRewinding else { return }
        isRewinding = false
        engine.setRewinding(rewinding: false)
    }

    /// Releases both held controls.
    ///
    /// Called when the player screen goes away or a game stops. Without it, leaving the player
    /// mid-hold would leave the engine fast-forwarding or rewinding with no button on screen
    /// to stop it, and the next game would inherit that.
    func releaseHeldControls() {
        endFastForward()
        endRewind()
    }

    // MARK: Readout

    /// Refreshes `rewindReadout` from the engine. Call when the Settings screen appears.
    ///
    /// Reports what the budget actually bought on the running game, because that is the number
    /// that cannot be stated in advance. With no game running there is no state size to divide
    /// by, so it says what it can and no more.
    func refreshRewindReadout() {
        guard rewindEnabled else {
            rewindReadout = "off"
            return
        }
        let stats = engine.rewindStats()
        let stateSize = engine.saveStateSize()
        let budgetMB = Double(stats.budgetBytes) / 1_048_576
        let heldMB = Double(stats.bytes) / 1_048_576

        guard stateSize > 0 else {
            rewindReadout = String(
                format: "%.0f MB set aside. Nothing recorded yet: start a game.", budgetMB)
            return
        }

        // Seconds of history the budget can hold, from this core's real state size.
        let snapshotsThatFit = Double(stats.budgetBytes) / Double(stateSize)
        let secondsPerSnapshot = Double(stats.intervalFrames) / 60.0
        let capacitySeconds = snapshotsThatFit * secondsPerSnapshot
        let heldSeconds = Double(stats.snapshots) * secondsPerSnapshot

        // The integers are interpolated rather than passed to `String(format:)`. A `UInt32`
        // handed to `%llu` is a varargs width mismatch: the formatter reads eight bytes for a
        // four-byte argument and prints whatever was next on the stack, which is a bug that
        // shows up as a plausible-looking wrong number rather than as a crash.
        let held = String(format: "%.1f", heldSeconds)
        let capacity = String(format: "%.0f", capacitySeconds)
        let usedMB = String(format: "%.1f", heldMB)
        let totalMB = String(format: "%.0f", budgetMB)
        rewindReadout = "\(held) s available of about \(capacity) s. "
            + "\(usedMB) MB of \(totalMB) MB used, "
            + "\(stats.snapshots) snapshots at \(stateSize / 1024) KB each."
    }

    /// One line for the diagnostics HUD. Short, because it shares a strip.
    var diagnosticLine: String {
        var parts: [String] = []
        parts.append("fit \(screenFit.label.lowercased())")
        parts.append(pixelFilter == .sharp ? "sharp" : "smooth")
        if isFastForwarding {
            parts.append("FF \(fastForward.label)")
        }
        if isRewinding {
            parts.append("REWINDING")
        }
        if muted {
            parts.append("muted")
        } else {
            parts.append("vol \(Int((volume * 100).rounded()))%")
        }
        if rewindEnabled {
            let stats = engine.rewindStats()
            parts.append("rewind \(stats.snapshots) snap")
        } else {
            parts.append("rewind off")
        }
        return parts.joined(separator: " | ")
    }
}

private extension Double {
    /// Guards against a stored value from an older build, or a corrupted one, landing outside
    /// the range a slider can represent.
    func clamped(to limits: ClosedRange<Double>) -> Double {
        min(max(self, limits.lowerBound), limits.upperBound)
    }
}
