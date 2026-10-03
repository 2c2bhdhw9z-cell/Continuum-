// Continuum - the feel of the controls: a tap on every on-screen press, and a core's rumble.
//
// Two different things that both end in the Taptic Engine, kept apart on purpose:
//
//   - BUTTON HAPTICS are this app's own idea. A short impact when a finger lands on an on-screen
//     control or slides into a new D-pad direction, so a glass pad can be played without looking.
//     Generated here, in Swift, because it is a property of a touch screen and not of a game.
//   - RUMBLE is the GAME's idea. A core asks for it through RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
//     the engine remembers the strengths (`input/rumble.rs`), and `RumblePlayer` polls them once a
//     frame and plays them on the phone and on any controller that has motors.
//
// The rumble DECISION (what was asked for, whether it is allowed) lives in Rust so an Android
// build gets it for free. Only the motor driver is here, because the motor is a platform API.

import CoreHaptics
import Foundation
import GameController
import UIKit

// MARK: - Settings

/// How hard an on-screen press taps back.
enum ButtonHapticStrength: String, CaseIterable, Identifiable {
    case off
    case light
    case medium
    case strong

    var id: String { rawValue }

    var label: String {
        switch self {
        case .off: return "Off"
        case .light: return "Light"
        case .medium: return "Medium"
        case .strong: return "Strong"
        }
    }

    /// The generator style for this strength. Nil for off, which builds no generator at all.
    var style: UIImpactFeedbackGenerator.FeedbackStyle? {
        switch self {
        case .off: return nil
        case .light: return .light
        case .medium: return .medium
        case .strong: return .heavy
        }
    }
}

/// How fast a turbo button pulses, as core frames down and then up.
///
/// The engine does the pulsing (`GamepadBridge::turbo_step`), so this is only the number it is
/// told. Counted in core frames so the rate holds at 120 Hz and under fast forward.
enum TurboRate: String, CaseIterable, Identifiable {
    case slow
    case normal
    case fast

    var id: String { rawValue }

    var label: String {
        switch self {
        case .slow: return "Slow"
        case .normal: return "Normal"
        case .fast: return "Fast"
        }
    }

    /// Half a cycle, in core frames. 8 is 3.75 presses a second at 60 Hz, 4 is 7.5 (the engine's
    /// own default), 2 is 15. Faster than 2 and many games stop seeing the release at all.
    var halfPeriodFrames: UInt32 {
        switch self {
        case .slow: return 8
        case .normal: return 4
        case .fast: return 2
        }
    }
}

/// The three feel settings, persisted. One shared instance, because the pad (a UIKit view deep
/// under SwiftUI) and the settings screen both need it and neither owns the other.
@MainActor
final class ControlFeel: ObservableObject {
    static let shared = ControlFeel()

    @Published var buttonHaptics: ButtonHapticStrength {
        didSet {
            UserDefaults.standard.set(buttonHaptics.rawValue, forKey: Self.buttonKey)
            ButtonHaptics.shared.setStrength(buttonHaptics)
        }
    }

    /// Whether a game's rumble reaches the motors. Pushed to the engine by `RumblePlayer`, so a
    /// core is told "not honoured" rather than having its requests silently dropped.
    @Published var rumbleEnabled: Bool {
        didSet { UserDefaults.standard.set(rumbleEnabled, forKey: Self.rumbleKey) }
    }

    @Published var turboRate: TurboRate {
        didSet { UserDefaults.standard.set(turboRate.rawValue, forKey: Self.turboKey) }
    }

    private static let buttonKey = "continuum.controls.buttonHaptics.v1"
    private static let rumbleKey = "continuum.controls.rumble.v1"
    private static let turboKey = "continuum.controls.turboRate.v1"

    private init() {
        let defaults = UserDefaults.standard
        buttonHaptics = defaults.string(forKey: Self.buttonKey)
            .flatMap(ButtonHapticStrength.init(rawValue:)) ?? .light
        rumbleEnabled = (defaults.object(forKey: Self.rumbleKey) as? Bool) ?? true
        turboRate = defaults.string(forKey: Self.turboKey)
            .flatMap(TurboRate.init(rawValue:)) ?? .normal
        ButtonHaptics.shared.setStrength(buttonHaptics)
    }
}

// MARK: - Button taps

/// The tap on an on-screen press.
///
/// THE GENERATOR IS PREPARED AHEAD, and re-prepared after every tap. `prepare()` spins the Taptic
/// Engine up so the next `impactOccurred` lands within a frame; an unprepared generator can take
/// long enough that the tap arrives after the button visibly lit, which feels like lag rather than
/// feedback. Only touched from the main thread, which is where every UIKit touch callback runs.
@MainActor
final class ButtonHaptics {
    static let shared = ButtonHaptics()

    private var generator: UIImpactFeedbackGenerator?
    private var strength: ButtonHapticStrength = .off
    /// The last tap, so a burst of presses in one touch event (two thumbs landing together) is
    /// one tap rather than a buzz.
    private var lastTap: CFTimeInterval = 0

    func setStrength(_ next: ButtonHapticStrength) {
        strength = next
        guard let style = next.style else {
            generator = nil
            return
        }
        let made = UIImpactFeedbackGenerator(style: style)
        made.prepare()
        generator = made
    }

    /// Re-arms the engine. Called when the pad appears, so the very first press is not late.
    func prepare() {
        generator?.prepare()
    }

    /// One press landed. Ignored when the setting is Off.
    func tap() {
        guard let generator else { return }
        let now = CACurrentMediaTime()
        guard now - lastTap > 0.03 else { return }
        lastTap = now
        generator.impactOccurred()
        generator.prepare()
    }
}

// MARK: - Rumble

/// One set of motors driven by Core Haptics: the phone's Taptic Engine, or a controller's.
///
/// A single looping continuous event whose intensity and sharpness are steered with dynamic
/// parameters, rather than a new pattern per change. A core can change its motors every frame,
/// and building and starting a pattern sixty times a second would stutter; steering one already
/// running player does not.
private final class HapticMotor {
    private let makeEngine: () -> CHHapticEngine?
    private var engine: CHHapticEngine?
    private var player: CHHapticAdvancedPatternPlayer?
    private var playing = false
    private(set) var lastFailure: String?

    init(makeEngine: @escaping () -> CHHapticEngine?) {
        self.makeEngine = makeEngine
    }

    /// `strong` and `weak` are the engine's `0...1` fractions.
    func play(strong: Float, weak: Float) {
        let intensity = max(strong, weak)
        guard intensity > 0.01 else {
            stop()
            return
        }
        // A strong motor is a low, heavy rumble; a weak one is a light buzz. Sharpness is how
        // Core Haptics says that, so it follows the share of the weak motor.
        let total = max(strong + weak, 0.0001)
        let sharpness = 0.15 + 0.6 * (weak / total)
        do {
            let player = try ensurePlayer()
            try player.sendParameters([
                CHHapticDynamicParameter(parameterID: .hapticIntensityControl,
                                         value: intensity, relativeTime: 0),
                CHHapticDynamicParameter(parameterID: .hapticSharpnessControl,
                                         value: sharpness, relativeTime: 0),
            ], atTime: CHHapticTimeImmediate)
            if !playing {
                try player.start(atTime: CHHapticTimeImmediate)
                playing = true
            }
            lastFailure = nil
        } catch {
            lastFailure = "\(error.localizedDescription)"
            // Dropped, so the next change rebuilds from scratch. An engine stopped by the system
            // (backgrounding, an audio interruption) throws here and is otherwise dead for good.
            self.player = nil
            self.engine = nil
            playing = false
        }
    }

    func stop() {
        guard playing else { return }
        try? player?.stop(atTime: CHHapticTimeImmediate)
        playing = false
    }

    private func ensurePlayer() throws -> CHHapticAdvancedPatternPlayer {
        if let player { return player }
        guard let engine = engine ?? makeEngine() else {
            throw NSError(domain: "ContinuumRumble", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "no haptic engine"])
        }
        engine.isAutoShutdownEnabled = true
        engine.stoppedHandler = { [weak self] _ in
            DispatchQueue.main.async {
                self?.player = nil
                self?.engine = nil
                self?.playing = false
            }
        }
        engine.resetHandler = { [weak self] in
            DispatchQueue.main.async {
                self?.player = nil
                self?.playing = false
            }
        }
        try engine.start()
        let event = CHHapticEvent(
            eventType: .hapticContinuous,
            parameters: [
                CHHapticEventParameter(parameterID: .hapticIntensity, value: 1),
                CHHapticEventParameter(parameterID: .hapticSharpness, value: 0.5),
            ],
            relativeTime: 0,
            // Long, and looped below. The dynamic parameters do the work; the event only has to
            // still be sounding when they arrive.
            duration: 30
        )
        let pattern = try CHHapticPattern(events: [event], parameters: [])
        let made = try engine.makeAdvancedPlayer(with: pattern)
        made.loopEnabled = true
        self.engine = engine
        self.player = made
        return made
    }
}

/// Polls the engine's rumble table once a frame and plays it.
///
/// Driven from the display link's telemetry callback, which is the main thread. `rumbleState`
/// takes no engine lock, so polling it every frame is a copy of three numbers.
@MainActor
final class RumblePlayer {
    static let shared = RumblePlayer()

    private let phone = HapticMotor {
        guard CHHapticEngine.capabilitiesForHardware().supportsHaptics else { return nil }
        return try? CHHapticEngine()
    }
    /// Controllers with motors, keyed by identity, so a pad that disconnects is forgotten.
    private var pads: [ObjectIdentifier: HapticMotor] = [:]
    private var lastGeneration: UInt64 = .max
    private var lastActive = false
    private var pushedEnabled: Bool?
    private var pushedTurbo: UInt32?

    /// Called every displayed frame. `active` is false while paused or with no game, which stops
    /// the motors even if the core never sent its zero.
    func poll(engine: ContinuumEngine, active: Bool) {
        let feel = ControlFeel.shared
        // The two settings the engine has to hear about, pushed only when they change.
        if pushedEnabled != feel.rumbleEnabled {
            engine.setRumbleEnabled(enabled: feel.rumbleEnabled)
            pushedEnabled = feel.rumbleEnabled
        }
        let turbo = feel.turboRate.halfPeriodFrames
        if pushedTurbo != turbo {
            engine.setTurboHalfPeriod(frames: turbo)
            pushedTurbo = turbo
        }

        guard active, feel.rumbleEnabled else {
            if lastActive {
                stopAll()
                lastActive = false
            }
            return
        }
        let phoneReading = engine.rumbleState(port: 0)
        if phoneReading.generation == lastGeneration, lastActive {
            return
        }
        lastGeneration = phoneReading.generation
        lastActive = true

        // The phone plays port 0, which is the player holding it.
        phone.play(strong: phoneReading.strong, weak: phoneReading.weak)

        // Each controller plays its own port, in the order they attached, which is the order
        // `PhysicalControllers` hands out ports.
        let controllers = GCController.controllers().filter { $0.extendedGamepad != nil }
        var seen = Set<ObjectIdentifier>()
        for (index, controller) in controllers.prefix(4).enumerated() {
            guard let haptics = controller.haptics else { continue }
            let key = ObjectIdentifier(controller)
            seen.insert(key)
            let motor: HapticMotor
            if let existing = pads[key] {
                motor = existing
            } else {
                motor = HapticMotor { haptics.createEngine(withLocality: .default) }
                pads[key] = motor
            }
            let reading = index == 0 ? phoneReading : engine.rumbleState(port: UInt32(index))
            motor.play(strong: reading.strong, weak: reading.weak)
        }
        for key in pads.keys where !seen.contains(key) {
            pads[key]?.stop()
            pads.removeValue(forKey: key)
        }
    }

    func stopAll() {
        phone.stop()
        for motor in pads.values { motor.stop() }
    }
}
