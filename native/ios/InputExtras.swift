// Continuum, the input extras: keyboards, motion, controller types, button mapping, and the
// console actions (shake, PlayStation analog, DS lid, blow, 3DS HOME).
//
// THE ENGINE OWNS ALL OF THE BEHAVIOUR. Remapping, profiles, the controller type chosen per port,
// the keyboard state and the keyboard callback, the sensor interface, the shake burst, how a DS
// lid or a 3DS HOME press reaches each core: all of that is Rust (crates/emulator-bridge/src/input,
// exported in uniffi_input.rs), so an Android host gets it by calling the same methods. This file
// is the iPhone side only: it reads GCKeyboard, UIPress and CoreMotion, draws the on-screen
// keyboard and the Controllers screen, and stores the engine's configuration text.
//
// The THREAD RULES are the engine's: a motion sample is pushed from CoreMotion's own queue through
// `pushMotion`, which takes no engine lock; everything else here is main-thread, and a key press
// only QUEUES an event that the engine hands to the core on the core's thread inside the next
// frame, never from a UIKit handler.

import CoreMotion
import GameController
import SwiftUI
import UIKit

// MARK: - Keys

/// libretro's `enum retro_key` values this file names (libretro.h:529-701).
enum RetroKey {
    static let backspace: UInt32 = 8
    static let tab: UInt32 = 9
    static let enter: UInt32 = 13
    static let pause: UInt32 = 19
    static let escape: UInt32 = 27
    static let space: UInt32 = 32
    static let delete: UInt32 = 127
    static let up: UInt32 = 273
    static let down: UInt32 = 274
    static let right: UInt32 = 275
    static let left: UInt32 = 276
    static let insert: UInt32 = 277
    static let home: UInt32 = 278
    static let end: UInt32 = 279
    static let pageUp: UInt32 = 280
    static let pageDown: UInt32 = 281
    static let f1: UInt32 = 282
    static let numLock: UInt32 = 300
    static let capsLock: UInt32 = 301
    static let scrollLock: UInt32 = 302
    static let rShift: UInt32 = 303
    static let lShift: UInt32 = 304
    static let rCtrl: UInt32 = 305
    static let lCtrl: UInt32 = 306
    static let rAlt: UInt32 = 307
    static let lAlt: UInt32 = 308
    static let lSuper: UInt32 = 311
    static let rSuper: UInt32 = 312
    static let print: UInt32 = 316

    static func isModifier(_ key: UInt32) -> Bool {
        (rShift...rSuper).contains(key)
    }
}

/// USB HID keyboard usages (what `GCKeyCode` and `UIKeyboardHIDUsage` both are) to `retro_key`,
/// with the US-layout characters each types. One table for both input paths.
enum HIDKeys {
    struct Entry {
        let retro: UInt32
        let normal: Character?
        let shifted: Character?
    }

    static func entry(hid: Int) -> Entry? {
        table[hid]
    }

    /// The UTF-32 character a key types, given whether shift is held. 0 for none.
    static func character(hid: Int, shift: Bool, caps: Bool) -> UInt32 {
        guard let e = table[hid], let base = e.normal else { return 0 }
        let isLetter = base.isLetter
        let useShift = isLetter ? (shift != caps) : shift
        let ch = useShift ? (e.shifted ?? base) : base
        return ch.unicodeScalars.first.map { $0.value } ?? 0
    }

    private static let table: [Int: Entry] = {
        var t: [Int: Entry] = [:]
        // Letters a..z: 0x04..0x1D.
        for (i, scalar) in "abcdefghijklmnopqrstuvwxyz".unicodeScalars.enumerated() {
            let ch = Character(scalar)
            t[0x04 + i] = Entry(retro: scalar.value, normal: ch,
                                shifted: Character(ch.uppercased()))
        }
        // 1..9 then 0: 0x1E..0x27.
        let digits: [(Character, Character)] = [("1", "!"), ("2", "@"), ("3", "#"), ("4", "$"),
                                                ("5", "%"), ("6", "^"), ("7", "&"), ("8", "*"),
                                                ("9", "("), ("0", ")")]
        for (i, pair) in digits.enumerated() {
            t[0x1E + i] = Entry(retro: pair.0.unicodeScalars.first!.value,
                                normal: pair.0, shifted: pair.1)
        }
        func put(_ hid: Int, _ retro: UInt32, _ n: Character? = nil, _ s: Character? = nil) {
            t[hid] = Entry(retro: retro, normal: n, shifted: s)
        }
        put(0x28, RetroKey.enter, "\r")
        put(0x29, RetroKey.escape)
        put(0x2A, RetroKey.backspace, "\u{8}")
        put(0x2B, RetroKey.tab, "\t")
        put(0x2C, RetroKey.space, " ", " ")
        put(0x2D, 45, "-", "_")
        put(0x2E, 61, "=", "+")
        put(0x2F, 91, "[", "{")
        put(0x30, 93, "]", "}")
        put(0x31, 92, "\\", "|")
        put(0x32, 92, "\\", "|")
        put(0x33, 59, ";", ":")
        put(0x34, 39, "'", "\"")
        put(0x35, 96, "`", "~")
        put(0x36, 44, ",", "<")
        put(0x37, 46, ".", ">")
        put(0x38, 47, "/", "?")
        put(0x39, RetroKey.capsLock)
        for i in 0..<12 { put(0x3A + i, RetroKey.f1 + UInt32(i)) }
        put(0x46, RetroKey.print)
        put(0x47, RetroKey.scrollLock)
        put(0x48, RetroKey.pause)
        put(0x49, RetroKey.insert)
        put(0x4A, RetroKey.home)
        put(0x4B, RetroKey.pageUp)
        put(0x4C, RetroKey.delete)
        put(0x4D, RetroKey.end)
        put(0x4E, RetroKey.pageDown)
        put(0x4F, RetroKey.right)
        put(0x50, RetroKey.left)
        put(0x51, RetroKey.down)
        put(0x52, RetroKey.up)
        put(0x53, RetroKey.numLock)
        put(0x54, 267, "/", "/")
        put(0x55, 268, "*", "*")
        put(0x56, 269, "-", "-")
        put(0x57, 270, "+", "+")
        put(0x58, 271, "\r", "\r")
        for i in 0..<9 {
            let ch = Character(String(i + 1))
            put(0x59 + i, 257 + UInt32(i), ch, ch)
        }
        put(0x62, 256, "0", "0")
        put(0x63, 266, ".", ".")
        put(0x64, 323)
        put(0x67, 272, "=", "=")
        for i in 0..<3 { put(0x68 + i, 294 + UInt32(i)) }
        put(0xE0, RetroKey.lCtrl)
        put(0xE1, RetroKey.lShift)
        put(0xE2, RetroKey.lAlt)
        put(0xE3, RetroKey.lSuper)
        put(0xE4, RetroKey.rCtrl)
        put(0xE5, RetroKey.rShift)
        put(0xE6, RetroKey.rAlt)
        put(0xE7, RetroKey.rSuper)
        return t
    }()
}

// MARK: - The object

/// Everything this file keeps between frames. One per engine, reached as `host.inputExtras`.
@MainActor
final class InputExtras: ObservableObject {
    /// The on-screen keyboard is up.
    @Published var showingKeyboard = false
    /// The keyboard is drawn with Commodore labels (RUN/STOP, C=, RESTORE) rather than PC ones.
    @Published var commodoreLabels = false
    /// The Controllers screen is up, and whether it opened straight on the button mapping.
    @Published var showingControllers = false
    @Published var openOnBinding = false

    @Published var motionEnabled: Bool {
        didSet {
            UserDefaults.standard.set(motionEnabled, forKey: Self.motionKey)
            engine.setMotionEnabled(enabled: motionEnabled)
        }
    }
    @Published var invertTiltX: Bool {
        didSet { pushInvert() }
    }
    @Published var invertTiltY: Bool {
        didSet { pushInvert() }
    }

    /// Plain lines for the Controllers screen.
    @Published private(set) var motionLine = "motion: not started"
    @Published private(set) var keyboardLine = "keyboard: no hardware keyboard attached"

    let engine: ContinuumEngine
    private let motion = CMMotionManager()
    private let motionQueue: OperationQueue = {
        let q = OperationQueue()
        q.name = "continuum.motion"
        q.maxConcurrentOperationCount = 1
        return q
    }()
    private var motionRunning = false
    /// From `gameStarted` until `gameEnded`. The display link, and so `poll`, keeps running under
    /// the Library (the canvas is never torn down), and the engine only forgets a core's motion
    /// request when that core is unloaded, so without this gate a core still loaded after its game
    /// could keep CoreMotion sampling at 100 Hz behind the Library.
    private var gameRunning = false
    private var frameCounter = 0
    private var keyboardObservers: [NSObjectProtocol] = []
    /// Set by the host so this object can write the status line without holding the host.
    var report: ((String) -> Void)?

    private static let motionKey = "continuum.input.motionEnabled.v1"
    private static let invertXKey = "continuum.input.invertTiltX.v1"
    private static let invertYKey = "continuum.input.invertTiltY.v1"
    private static let configKey = "continuum.input.config.v1"

    private static var instances: [ObjectIdentifier: InputExtras] = [:]

    /// The one instance for an engine. A table rather than a stored property on `EngineHost`,
    /// so this file adds no field to that type.
    static func shared(for engine: ContinuumEngine) -> InputExtras {
        let key = ObjectIdentifier(engine)
        if let existing = instances[key] { return existing }
        let made = InputExtras(engine: engine)
        instances[key] = made
        return made
    }

    private init(engine: ContinuumEngine) {
        self.engine = engine
        let defaults = UserDefaults.standard
        motionEnabled = (defaults.object(forKey: Self.motionKey) as? Bool) ?? true
        invertTiltX = defaults.bool(forKey: Self.invertXKey)
        invertTiltY = defaults.bool(forKey: Self.invertYKey)
        // didSet does not run in init, so the engine is told here.
        engine.setMotionEnabled(enabled: motionEnabled)
        engine.setMotionInverted(invertX: invertTiltX, invertY: invertTiltY)
        if let text = defaults.string(forKey: Self.configKey) {
            _ = engine.importInputConfig(text: text)
        }
        observeKeyboards()
    }

    private func pushInvert() {
        UserDefaults.standard.set(invertTiltX, forKey: Self.invertXKey)
        UserDefaults.standard.set(invertTiltY, forKey: Self.invertYKey)
        engine.setMotionInverted(invertX: invertTiltX, invertY: invertTiltY)
    }

    /// Stores the engine's configuration. Called after anything that changes it.
    func persistConfig() {
        UserDefaults.standard.set(engine.exportInputConfig(), forKey: Self.configKey)
    }

    // MARK: Per frame

    /// From the display link after each tick. Starts and stops CoreMotion when the core's wish
    /// changes (an atomic read in Rust), keeps the screen orientation current, and carries out
    /// host-side actions a remapped button pressed.
    func poll() {
        let wanted = gameRunning && engine.motionWanted()
        if wanted != motionRunning {
            wanted ? startMotion() : stopMotion()
        }
        frameCounter += 1
        if frameCounter % 30 == 0 {
            pushOrientation()
            let line = engine.motionStatus()
            if line != motionLine { motionLine = line }
        }
        let host = engine.takeHostInputActions()
        if !host.line.isEmpty {
            report?(host.line)
            if host.line.hasPrefix("controls: switched") { persistConfig() }
        }
        for action in host.actions {
            switch action {
            case "keyboard":
                showingKeyboard.toggle()
                // Hidden through a remapped button: latched Shift, Ctrl and Alt are let go exactly
                // as the keyboard's own hide button lets go of them, or they stay held in the game.
                if !showingKeyboard { keyboardHidden() }
                report?(showingKeyboard ? "keyboard: shown" : "keyboard: hidden")
            case "menu":
                openOnBinding = false
                showingControllers = true
            default:
                report?("controls: the app does not know the action \"\(action)\"")
            }
        }
    }

    // MARK: Motion

    private func startMotion() {
        guard motion.isDeviceMotionAvailable || motion.isAccelerometerAvailable else {
            motionRunning = true // so this is not retried sixty times a second
            report?("motion: this device has no motion sensor, so tilt games cannot be tilted")
            return
        }
        pushOrientation()
        let rust = engine
        motionRunning = true
        if motion.isDeviceMotionAvailable {
            motion.deviceMotionUpdateInterval = 1.0 / 100.0
            motion.startDeviceMotionUpdates(to: motionQueue) { data, _ in
                guard let data else { return }
                // libretro's sign: a phone flat on a table reads (0, 0, +1). CoreMotion reads
                // gravity as (0, 0, -1), so the whole acceleration is negated. Rotation is
                // counter-clockwise positive in both, so it passes as is.
                let g = data.gravity
                let u = data.userAcceleration
                let r = data.rotationRate
                rust.pushMotion(ax: Float(-(g.x + u.x)), ay: Float(-(g.y + u.y)),
                                az: Float(-(g.z + u.z)),
                                gx: Float(r.x), gy: Float(r.y), gz: Float(r.z))
            }
        } else {
            motion.accelerometerUpdateInterval = 1.0 / 100.0
            motion.startAccelerometerUpdates(to: motionQueue) { data, _ in
                guard let a = data?.acceleration else { return }
                rust.pushMotion(ax: Float(-a.x), ay: Float(-a.y), az: Float(-a.z),
                                gx: 0, gy: 0, gz: 0)
            }
        }
        report?("motion: the game reads the motion sensor; tilt the phone")
    }

    private func stopMotion() {
        if motion.isDeviceMotionActive { motion.stopDeviceMotionUpdates() }
        if motion.isAccelerometerActive { motion.stopAccelerometerUpdates() }
        motionRunning = false
    }

    private func pushOrientation() {
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
        let code: UInt32
        switch scene?.interfaceOrientation ?? .portrait {
        case .landscapeRight: code = 1
        case .landscapeLeft: code = 2
        case .portraitUpsideDown: code = 3
        default: code = 0
        }
        engine.setMotionOrientation(orientation: code)
    }

    func calibrate() -> String {
        engine.calibrateMotion()
    }

    // MARK: Session

    /// A game has started. From here `poll` starts CoreMotion whenever the core asks for it.
    func gameStarted() {
        gameRunning = true
    }

    /// The game has ended. Stops CoreMotion now rather than leaving it to `poll`, lets go of the
    /// on-screen keyboard's latched Shift, Ctrl and Alt, and forgets caps lock (the next core
    /// starts with it off). Motion comes back normally for the next game that asks: it is marked
    /// stopped here, and `gameStarted` reopens the gate `poll` checks.
    func gameEnded() {
        gameRunning = false
        stopMotion()
        showingKeyboard = false
        keyboardHidden()
        capsOn = false
    }

    // MARK: Hardware keyboards

    private func observeKeyboards() {
        let centre = NotificationCenter.default
        keyboardObservers = [
            centre.addObserver(forName: .GCKeyboardDidConnect, object: nil,
                               queue: .main) { [weak self] _ in
                Task { @MainActor in self?.attachKeyboard() }
            },
            centre.addObserver(forName: .GCKeyboardDidDisconnect, object: nil,
                               queue: .main) { [weak self] _ in
                Task { @MainActor in
                    self?.keyboardLine = "keyboard: hardware keyboard disconnected"
                    self?.engine.releaseInputSource(source: .keyboard)
                }
            },
        ]
        attachKeyboard()
    }

    private func attachKeyboard() {
        guard let input = GCKeyboard.coalesced?.keyboardInput else { return }
        let rust = engine
        input.keyChangedHandler = { keyboard, _, keyCode, pressed in
            let hid = keyCode.rawValue
            guard let entry = HIDKeys.entry(hid: hid) else { return }
            let shift = (keyboard.button(forKeyCode: .leftShift)?.isPressed ?? false)
                || (keyboard.button(forKeyCode: .rightShift)?.isPressed ?? false)
            let character = pressed ? HIDKeys.character(hid: hid, shift: shift, caps: false) : 0
            rust.keyEvent(source: .keyboard, keycode: entry.retro, down: pressed,
                          character: character)
        }
        keyboardLine = "keyboard: hardware keyboard attached; keys go to the game"
    }

    /// One press from the UIKit responder path. Duplicates of a GCKeyboard press are harmless:
    /// the engine only reports a change of the merged state, so the second copy does nothing.
    func handlePress(hid: Int, characters: String, down: Bool) -> Bool {
        guard let entry = HIDKeys.entry(hid: hid) else { return false }
        let character = down ? (characters.unicodeScalars.first.map { $0.value } ?? 0) : 0
        engine.keyEvent(source: .keyboard, keycode: entry.retro, down: down, character: character)
        return true
    }

    // MARK: On-screen keyboard

    /// Sticky modifiers on the on-screen keyboard: tap Shift, then a letter.
    @Published var stickyShift = false
    @Published var stickyCtrl = false
    @Published var stickyAlt = false
    @Published var capsOn = false

    func onScreenKey(_ key: OnScreenKey, down: Bool) {
        if RetroKey.isModifier(key.retro) {
            // Modifiers latch on tap and are sent as held until the next ordinary key lets go.
            guard down else { return }
            switch key.retro {
            case RetroKey.lShift, RetroKey.rShift: stickyShift.toggle()
            case RetroKey.lCtrl, RetroKey.rCtrl: stickyCtrl.toggle()
            default: stickyAlt.toggle()
            }
            engine.keyEvent(source: .touch, keycode: key.retro,
                            down: isLatched(key.retro), character: 0)
            return
        }
        if key.retro == RetroKey.capsLock, down { capsOn.toggle() }
        let character: UInt32
        if down, let base = key.character {
            let letter = base.isLetter
            let upper = letter ? (stickyShift != capsOn) : stickyShift
            let ch = upper ? (key.shifted ?? Character(base.uppercased())) : base
            character = ch.unicodeScalars.first.map { $0.value } ?? 0
        } else {
            character = 0
        }
        engine.keyEvent(source: .touch, keycode: key.retro, down: down, character: character)
        if !down { releaseSticky() }
    }

    private func isLatched(_ retro: UInt32) -> Bool {
        switch retro {
        case RetroKey.lShift, RetroKey.rShift: return stickyShift
        case RetroKey.lCtrl, RetroKey.rCtrl: return stickyCtrl
        default: return stickyAlt
        }
    }

    private func releaseSticky() {
        if stickyShift {
            stickyShift = false
            engine.keyEvent(source: .touch, keycode: RetroKey.lShift, down: false, character: 0)
        }
        if stickyCtrl {
            stickyCtrl = false
            engine.keyEvent(source: .touch, keycode: RetroKey.lCtrl, down: false, character: 0)
        }
        if stickyAlt {
            stickyAlt = false
            engine.keyEvent(source: .touch, keycode: RetroKey.lAlt, down: false, character: 0)
        }
    }

    /// The keyboard went away: nothing it holds may stay held.
    func keyboardHidden() {
        stickyShift = false
        stickyCtrl = false
        stickyAlt = false
        for key in [RetroKey.lShift, RetroKey.lCtrl, RetroKey.lAlt] {
            engine.keyEvent(source: .touch, keycode: key, down: false, character: 0)
        }
    }
}

// MARK: - The EngineHost methods other code calls by name

extension EngineHost {
    /// This engine's input extras. See `InputExtras.shared`.
    var inputExtras: InputExtras {
        let extras = InputExtras.shared(for: engine)
        if extras.report == nil {
            extras.report = { [weak self] line in self?.status = line }
        }
        return extras
    }

    /// Shared system id of the running game, from the launched file.
    var inputSystemId: String {
        activeSystem?.rawValue ?? ""
    }

    /// Call on every successful launch. Applies the remap profile and the controller types the
    /// user chose for this system, and says so.
    func inputSessionDidStart() {
        let line = engine.inputSessionStarted(system: inputSystemId,
                                              game: activeEntry?.name ?? "")
        let extras = inputExtras
        extras.showingKeyboard = false
        extras.commodoreLabels = ["c64", "amiga"].contains(inputSystemId)
        extras.gameStarted()
        NSLog("[continuum] %@", line)
    }

    /// Call from `stopSession`, before `engine.stop()`. Stops CoreMotion and lets go of the
    /// on-screen keyboard's latched keys; see `InputExtras.gameEnded`.
    func inputSessionDidEnd() {
        inputExtras.gameEnded()
    }

    /// Once per display-link frame, after the tick.
    func inputFramePoll() {
        inputExtras.poll()
    }

    @discardableResult
    private func inputReport(_ line: String) -> String {
        status = line
        return line
    }

    /// A short shake: the Pokemon Mini's shake, or an accelerometer burst for any other core.
    @discardableResult
    func shake() -> String {
        inputReport(engine.shake())
    }

    /// PlayStation: digital pad to DualShock and back.
    @discardableResult
    func toggleAnalogMode() -> String {
        inputReport(engine.toggleAnalogMode())
    }

    func isAnalogMode() -> Bool {
        engine.isAnalogMode()
    }

    /// Nintendo DS: close the lid, or open it.
    @discardableResult
    func toggleDSLid() -> String {
        inputReport(engine.toggleDsLid())
    }

    /// Nintendo DS: blow into the microphone while held.
    @discardableResult
    func blowIntoMic(held: Bool) -> String {
        inputReport(engine.blowIntoMic(held: held))
    }

    /// Nintendo 3DS: one press of HOME.
    @discardableResult
    func pressHomeButton() -> String {
        inputReport(engine.pressHomeButton())
    }

    /// Opens the Controllers screen.
    @discardableResult
    func showControllers() -> String {
        let extras = inputExtras
        extras.openOnBinding = false
        extras.showingControllers = true
        return inputReport("controls: Controllers opened")
    }

    /// Next remap profile for this system (Manic EMU's triggerPro).
    @discardableResult
    func cycleTriggerProfile() -> String {
        let line = engine.cycleInputProfile()
        inputExtras.persistConfig()
        return inputReport(line)
    }

    /// Opens the Controllers screen on the button mapping for the running system.
    @discardableResult
    func showButtonBinding() -> String {
        guard !inputSystemId.isEmpty else {
            return inputReport("controls: start a game to map its buttons, or use Settings")
        }
        let extras = inputExtras
        extras.openOnBinding = true
        extras.showingControllers = true
        return inputReport("controls: button mapping opened for \(inputSystemId)")
    }

    /// Shows or hides the on-screen keyboard.
    @discardableResult
    func toggleOnScreenKeyboard() -> String {
        let extras = inputExtras
        extras.showingKeyboard.toggle()
        if !extras.showingKeyboard { extras.keyboardHidden() }
        return inputReport(extras.showingKeyboard ? "keyboard: shown" : "keyboard: hidden")
    }
}

// MARK: - On-screen keyboard

struct OnScreenKey: Identifiable, Hashable {
    let id: String
    let label: String
    let commodore: String?
    let retro: UInt32
    let character: Character?
    let shifted: Character?
    /// Width in key units.
    let width: CGFloat

    init(_ label: String, _ retro: UInt32, _ character: Character? = nil,
         _ shifted: Character? = nil, width: CGFloat = 1, c64: String? = nil) {
        self.id = "\(label)-\(retro)"
        self.label = label
        self.commodore = c64
        self.retro = retro
        self.character = character
        self.shifted = shifted
        self.width = width
    }
}

enum OnScreenKeyboardLayout {
    static func letters(_ s: String) -> [OnScreenKey] {
        s.map { ch in OnScreenKey(String(ch).uppercased(), ch.unicodeScalars.first!.value, ch) }
    }

    /// A full PC keyboard, compressed to six rows. The Commodore names are where VICE puts those
    /// keys on a PC keyboard: RUN/STOP is Escape and RESTORE is Page Up.
    static let rows: [[OnScreenKey]] = [
        [OnScreenKey("Esc", RetroKey.escape, width: 1.3, c64: "RUN/STOP")]
            + (1...10).map { OnScreenKey("F\($0)", RetroKey.f1 + UInt32($0 - 1)) }
            + [OnScreenKey("Del", RetroKey.delete, width: 1.2)],
        [OnScreenKey("`", 96, "`", "~")]
            + [("1", "!"), ("2", "@"), ("3", "#"), ("4", "$"), ("5", "%"), ("6", "^"),
               ("7", "&"), ("8", "*"), ("9", "("), ("0", ")")].map { pair in
                OnScreenKey(pair.0, pair.0.unicodeScalars.first!.value,
                            Character(pair.0), Character(pair.1))
            }
            + [OnScreenKey("-", 45, "-", "_"), OnScreenKey("=", 61, "=", "+"),
               OnScreenKey("⌫", RetroKey.backspace, width: 1.5, c64: "INST/DEL")],
        [OnScreenKey("Tab", RetroKey.tab, "\t", width: 1.4, c64: "CTRL")]
            + letters("qwertyuiop")
            + [OnScreenKey("[", 91, "[", "{"), OnScreenKey("]", 93, "]", "}"),
               OnScreenKey("\\", 92, "\\", "|")],
        [OnScreenKey("Caps", RetroKey.capsLock, width: 1.6)]
            + letters("asdfghjkl")
            + [OnScreenKey(";", 59, ";", ":"), OnScreenKey("'", 39, "'", "\""),
               OnScreenKey("Enter", RetroKey.enter, "\r", width: 1.9, c64: "RETURN")],
        [OnScreenKey("Shift", RetroKey.lShift, width: 2.0)]
            + letters("zxcvbnm")
            + [OnScreenKey(",", 44, ",", "<"), OnScreenKey(".", 46, ".", ">"),
               OnScreenKey("/", 47, "/", "?"), OnScreenKey("↑", RetroKey.up),
               OnScreenKey("PgUp", RetroKey.pageUp, c64: "RESTORE")],
        [OnScreenKey("Ctrl", RetroKey.lCtrl, width: 1.4, c64: "C="),
         OnScreenKey("Alt", RetroKey.lAlt, width: 1.2),
         OnScreenKey("Ins", RetroKey.insert),
         OnScreenKey("Space", RetroKey.space, " ", " ", width: 4.6),
         OnScreenKey("Home", RetroKey.home, width: 1.2, c64: "CLR/HOME"),
         OnScreenKey("End", RetroKey.end),
         OnScreenKey("←", RetroKey.left), OnScreenKey("↓", RetroKey.down),
         OnScreenKey("→", RetroKey.right)],
    ]
}

/// The on-screen keyboard: six rows across the bottom of the player, about a third of a portrait
/// screen and less in landscape, so the picture above it stays visible. Translucent, so what it
/// does cover is not hidden outright.
struct OnScreenKeyboardView: View {
    @ObservedObject var extras: InputExtras
    let onClose: () -> Void

    var body: some View {
        GeometryReader { proxy in
            let landscape = proxy.size.width > proxy.size.height
            let rowHeight: CGFloat = landscape ? 30 : 38
            VStack(spacing: 4) {
                HStack {
                    Text(extras.commodoreLabels ? "Commodore keys" : "Keyboard")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(Color.white.opacity(0.6))
                    Spacer()
                    Button(extras.commodoreLabels ? "PC labels" : "C64 labels") {
                        extras.commodoreLabels.toggle()
                    }
                    .font(.system(size: 11, weight: .semibold))
                    Button {
                        onClose()
                    } label: {
                        Image(systemName: "keyboard.chevron.compact.down")
                    }
                    .accessibilityLabel("Hide the keyboard")
                }
                .padding(.horizontal, 6)
                ForEach(0..<OnScreenKeyboardLayout.rows.count, id: \.self) { index in
                    keyRow(OnScreenKeyboardLayout.rows[index], width: proxy.size.width,
                           height: rowHeight)
                }
            }
            .padding(.vertical, 6)
            .background(Color.black.opacity(0.72), in: RoundedRectangle(cornerRadius: 12))
            .frame(maxHeight: .infinity, alignment: .bottom)
        }
    }

    private func keyRow(_ keys: [OnScreenKey], width: CGFloat, height: CGFloat) -> some View {
        let units = keys.reduce(0) { $0 + $1.width }
        let unit = max(10, (width - 8 - CGFloat(keys.count - 1) * 3) / units)
        return HStack(spacing: 3) {
            ForEach(keys) { key in
                KeyCap(key: key, extras: extras, width: unit * key.width, height: height)
            }
        }
        .frame(maxWidth: .infinity)
    }
}

/// One key. A press-and-release gesture rather than a Button, so a held key is held (a game that
/// polls the keyboard sees it down for as long as the finger is).
private struct KeyCap: View {
    let key: OnScreenKey
    @ObservedObject var extras: InputExtras
    let width: CGFloat
    let height: CGFloat
    @State private var pressed = false

    private var latched: Bool {
        switch key.retro {
        case RetroKey.lShift: return extras.stickyShift
        case RetroKey.lCtrl: return extras.stickyCtrl
        case RetroKey.lAlt: return extras.stickyAlt
        case RetroKey.capsLock: return extras.capsOn
        default: return false
        }
    }

    var body: some View {
        let label = extras.commodoreLabels ? (key.commodore ?? key.label) : key.label
        Text(label)
            .font(.system(size: label.count > 4 ? 9 : 13, weight: .semibold))
            .minimumScaleFactor(0.5)
            .lineLimit(1)
            .foregroundStyle(.white)
            .frame(width: width, height: height)
            .background((pressed || latched) ? ShellPalette.accent.opacity(0.85)
                                             : Color.white.opacity(0.16),
                        in: RoundedRectangle(cornerRadius: 5))
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        if !pressed {
                            pressed = true
                            extras.onScreenKey(key, down: true)
                        }
                    }
                    .onEnded { _ in
                        pressed = false
                        extras.onScreenKey(key, down: false)
                    }
            )
            .accessibilityLabel(label)
    }
}

// MARK: - UIPress capture

/// Becomes first responder in the player so a hardware keyboard's presses reach the game through
/// UIKit as well as through GCKeyboard (some keyboards, and some iOS versions, only deliver one).
final class KeyCaptureView: UIView {
    weak var extras: InputExtras?

    override var canBecomeFirstResponder: Bool { true }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            DispatchQueue.main.async { [weak self] in _ = self?.becomeFirstResponder() }
        }
    }

    private func handle(_ presses: Set<UIPress>, down: Bool) -> Set<UIPress> {
        var unhandled = Set<UIPress>()
        for press in presses {
            guard let key = press.key,
                  let extras,
                  extras.handlePress(hid: key.keyCode.rawValue, characters: key.characters,
                                     down: down)
            else {
                unhandled.insert(press)
                continue
            }
        }
        return unhandled
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        let rest = handle(presses, down: true)
        if !rest.isEmpty { super.pressesBegan(rest, with: event) }
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        let rest = handle(presses, down: false)
        if !rest.isEmpty { super.pressesEnded(rest, with: event) }
    }

    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        let rest = handle(presses, down: false)
        if !rest.isEmpty { super.pressesCancelled(rest, with: event) }
    }
}

struct KeyCaptureHost: UIViewRepresentable {
    let extras: InputExtras

    func makeUIView(context: Context) -> KeyCaptureView {
        let view = KeyCaptureView()
        view.extras = extras
        view.isUserInteractionEnabled = false
        return view
    }

    func updateUIView(_ uiView: KeyCaptureView, context: Context) {
        uiView.extras = extras
    }

    static func dismantleUIView(_ uiView: KeyCaptureView, coordinator: Coordinator) {
        uiView.resignFirstResponder()
        uiView.extras?.engine.releaseInputSource(source: .keyboard)
    }
}

// MARK: - The player layer

/// Everything this file puts over the player: the hardware key capture, the keyboard button for
/// the computers, the on-screen keyboard, and the Controllers sheet. Mounted once in PlayerScreen.
struct InputPlayerLayer: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var extras: InputExtras
    let system: GameSystem?
    @State private var blowing = false
    /// Compact in landscape on an iPhone, which is where the keyboard has to be shorter.
    @Environment(\.verticalSizeClass) private var verticalSizeClass

    init(host: EngineHost, extras: InputExtras, system: GameSystem?) {
        self.host = host
        self.extras = extras
        self.system = system
    }

    private var isComputer: Bool {
        ["dos", "c64", "amiga"].contains(system?.rawValue ?? "")
    }

    var body: some View {
        ZStack(alignment: .bottom) {
            KeyCaptureHost(extras: extras)
                .frame(width: 1, height: 1)
                .allowsHitTesting(false)

            if isComputer && !extras.showingKeyboard {
                HStack {
                    Spacer()
                    Button {
                        host.toggleOnScreenKeyboard()
                    } label: {
                        Image(systemName: "keyboard")
                            .font(.system(size: 15, weight: .semibold))
                            .frame(width: 38, height: 38)
                            .background(Color.white.opacity(0.18), in: Circle())
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white)
                    .accessibilityLabel("Show the keyboard")
                }
                .padding(.trailing, 12)
                // Just under the top bar, clear of the pad and of a DS touch screen.
                .padding(.top, 58)
                .frame(maxHeight: .infinity, alignment: .top)
            }

            // The DS microphone: held, because melonDS blows for as long as its button is down.
            if system == .ds {
                HStack {
                    Image(systemName: blowing ? "mic.fill" : "mic")
                        .font(.system(size: 15, weight: .semibold))
                        .frame(width: 38, height: 38)
                        .background(blowing ? ShellPalette.accent.opacity(0.85)
                                            : Color.white.opacity(0.18), in: Circle())
                        .foregroundStyle(.white)
                        .contentShape(Circle())
                        .gesture(
                            DragGesture(minimumDistance: 0)
                                .onChanged { _ in
                                    if !blowing {
                                        blowing = true
                                        host.blowIntoMic(held: true)
                                    }
                                }
                                .onEnded { _ in
                                    blowing = false
                                    host.blowIntoMic(held: false)
                                }
                        )
                        .accessibilityLabel("Blow into the microphone")
                    Spacer()
                }
                .padding(.leading, 12)
                // Just under the top bar, clear of the pad and of a DS touch screen.
                .padding(.top, 58)
                .frame(maxHeight: .infinity, alignment: .top)
            }

            if extras.showingKeyboard {
                OnScreenKeyboardView(extras: extras) {
                    host.toggleOnScreenKeyboard()
                }
                .frame(height: keyboardHeight)
                .padding(.horizontal, 4)
                .padding(.bottom, 4)
                .transition(.move(edge: .bottom))
            }
        }
        .sheet(isPresented: $extras.showingControllers) {
            ControllersScreen(host: host, extras: extras, controllers: host.controllers,
                              startOnBinding: extras.openOnBinding) {
                extras.showingControllers = false
            }
        }
    }

    /// Six rows plus a header. Measured rather than a fraction, so it never takes more than it
    /// needs: about 290 points in portrait, about 230 in landscape.
    private var keyboardHeight: CGFloat {
        verticalSizeClass == .compact ? 6 * 34 + 34 : 6 * 42 + 40
    }
}

// MARK: - Controllers screen

/// Connected controllers and their ports, the controller TYPE per port, the remap profiles and
/// their editor, and the motion settings. Opened from the player and from Settings.
struct ControllersScreen: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var extras: InputExtras
    @ObservedObject var controllers: PhysicalControllers
    let startOnBinding: Bool
    let onDone: () -> Void

    @State private var system: String = ""
    @State private var ports: [ControllerPortRecord] = []
    @State private var profiles: [RemapProfileRecord] = []
    @State private var active: String = ""
    @State private var editing: RemapProfileRecord?
    @State private var line: String = ""

    private var systems: [String] { GameSystem.allCases.map { $0.rawValue } }

    var body: some View {
        NavigationView {
            List {
                Section("Connected controllers") {
                    if controllers.portAssignments.isEmpty {
                        Text("No controller attached. Pair one in iOS Settings, then Bluetooth.")
                            .font(.footnote)
                    }
                    ForEach(controllers.portAssignments) { pad in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(pad.name).font(.body)
                                Text(pad.port.map { "Player \($0 + 1)" } ?? "Not on a port")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            if let port = pad.port {
                                Menu("Move") {
                                    ForEach(0..<PhysicalControllers.maxPorts, id: \.self) { to in
                                        if to != port {
                                            Button("Player \(to + 1)") {
                                                line = controllers.movePad(from: port, to: to)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Text("Profile in force: \(active.isEmpty ? "Default" : active)")
                        .font(.caption)
                }

                Section("System") {
                    Picker("System", selection: $system) {
                        ForEach(systems, id: \.self) { Text($0).tag($0) }
                    }
                    .onChange(of: system) { _ in reload() }
                }

                Section("Controller type") {
                    if system != host.inputSystemId || ports.allSatisfy({ $0.choices.isEmpty }) {
                        Text(system == host.inputSystemId
                             ? "This core offers no other controller types."
                             : "Start a \(system) game to see the types its core offers. A choice "
                                + "made then is remembered and applied at every launch.")
                            .font(.footnote)
                    }
                    ForEach(ports.filter { !$0.choices.isEmpty && system == host.inputSystemId },
                            id: \.port) { port in
                        Menu {
                            ForEach(port.choices, id: \.self) { choice in
                                Button(choice) {
                                    line = host.engine.setControllerType(system: system,
                                                                         port: port.port,
                                                                         name: choice)
                                    extras.persistConfig()
                                    reload()
                                }
                            }
                            Button("Core default") {
                                line = host.engine.setControllerType(system: system,
                                                                     port: port.port, name: "")
                                extras.persistConfig()
                                reload()
                            }
                        } label: {
                            HStack {
                                Text("Player \(port.port + 1)")
                                Spacer()
                                Text(port.current).foregroundStyle(.secondary)
                            }
                        }
                    }
                    if system == "ps1" && system == host.inputSystemId {
                        Button(host.isAnalogMode() ? "Switch to digital pad"
                                                   : "Switch to analog (DualShock)") {
                            line = host.toggleAnalogMode()
                            reload()
                        }
                    }
                }

                Section("Button mapping profiles") {
                    ForEach(Array(profiles.enumerated()), id: \.offset) { _, profile in
                        HStack {
                            Button {
                                line = host.engine.selectInputProfile(system: system,
                                                                      game: "",
                                                                      name: profile.name)
                                extras.persistConfig()
                                reload()
                            } label: {
                                HStack {
                                    Image(systemName: profile.name == active
                                          ? "checkmark.circle.fill" : "circle")
                                    Text(profile.name)
                                    if !profile.game.isEmpty {
                                        Text("(\(profile.game))").font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                            }
                            Spacer()
                            Button("Edit") { editing = profile }
                                .buttonStyle(.borderless)
                        }
                    }
                    Button("New profile") {
                        var fresh = host.engine.activeInputProfile(system: system, game: "")
                        fresh.name = "Profile \(profiles.count + 1)"
                        fresh.game = ""
                        editing = fresh
                    }
                    if !host.inputSystemId.isEmpty && system == host.inputSystemId {
                        Button("New profile for this game only") {
                            var fresh = host.engine.activeInputProfile(system: system, game: "")
                            fresh.name = host.activeEntry?.name ?? "This game"
                            fresh.game = host.activeEntry?.name ?? ""
                            editing = fresh
                        }
                        Button("Next profile (the cycle button)") {
                            line = host.cycleTriggerProfile()
                            reload()
                        }
                    }
                }

                Section("Motion") {
                    Toggle("Tilt and motion sensor", isOn: $extras.motionEnabled)
                    Toggle("Invert left and right tilt", isOn: $extras.invertTiltX)
                    Toggle("Invert forward and back tilt", isOn: $extras.invertTiltY)
                    Button("Calibrate: hold the phone how you play, then tap") {
                        line = extras.calibrate()
                    }
                    Button("Reset calibration to flat") {
                        host.engine.resetMotionCalibration()
                        line = "motion: calibration reset, flat on a table is level"
                    }
                    Button("Shake") { line = host.shake() }
                    Text(extras.motionLine).font(.caption).foregroundStyle(.secondary)
                }

                Section("Keyboard") {
                    Text(extras.keyboardLine).font(.caption)
                    Text(host.engine.keyboardStatus()).font(.caption).foregroundStyle(.secondary)
                }

                if !line.isEmpty {
                    Section { Text(line).font(.footnote) }
                }
            }
            .navigationTitle("Controllers")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
            .sheet(item: Binding(get: { editing.map(EditingBox.init) },
                                 set: { editing = $0?.profile })) { box in
                RemapEditor(host: host, profile: box.profile) { saved in
                    if let saved {
                        line = host.engine.saveInputProfile(profile: saved)
                        extras.persistConfig()
                    }
                    editing = nil
                    reload()
                } onDelete: { doomed in
                    line = host.engine.deleteInputProfile(system: doomed.system,
                                                          game: doomed.game,
                                                          name: doomed.name)
                    extras.persistConfig()
                    editing = nil
                    reload()
                }
            }
        }
        .onAppear {
            system = host.inputSystemId.isEmpty ? (systems.first ?? "nes") : host.inputSystemId
            reload()
            if startOnBinding {
                editing = host.engine.activeInputProfile(system: system,
                                                         game: host.activeEntry?.name ?? "")
            }
        }
    }

    private func reload() {
        ports = host.engine.controllerPorts()
        profiles = host.engine.inputProfiles(system: system)
        let game = system == host.inputSystemId ? (host.activeEntry?.name ?? "") : ""
        active = host.engine.activeInputProfile(system: system, game: game).name
    }
}

private struct EditingBox: Identifiable {
    let profile: RemapProfileRecord
    var id: String { profile.system + "/" + profile.game + "/" + profile.name }
}

/// The button mapping editor: every button of the controller and of the on-screen pad, each to
/// any game input, nothing, or an app action, plus swap A/B, stick to D-pad and the deadzone.
struct RemapEditor: View {
    let host: EngineHost
    @State var profile: RemapProfileRecord
    let onFinish: (RemapProfileRecord?) -> Void
    let onDelete: (RemapProfileRecord) -> Void

    @State private var layer = 0
    private var labels: [String] { host.engine.remapButtonLabels() }
    private var targets: [RemapTargetRecord] { host.engine.remapTargets() }

    var body: some View {
        NavigationView {
            Form {
                Section("Profile") {
                    TextField("Name", text: $profile.name)
                    Text(profile.game.isEmpty ? "Every \(profile.system) game"
                                              : "Only \(profile.game)")
                        .font(.caption)
                }
                Picker("Buttons of", selection: $layer) {
                    Text("Controller").tag(0)
                    Text("On-screen pad").tag(1)
                }
                .pickerStyle(.segmented)

                Section(layer == 0 ? "Controller button becomes" : "On-screen button becomes") {
                    ForEach(0..<labels.count, id: \.self) { index in
                        Picker(labels[index], selection: binding(index)) {
                            ForEach(targets, id: \.code) { target in
                                Text(target.label).tag(target.code)
                            }
                        }
                    }
                }

                Section("Options") {
                    Toggle("Swap A and B", isOn: layer == 0 ? $profile.gamepadSwapAb
                                                             : $profile.touchSwapAb)
                    Toggle("Left stick also moves the D-pad",
                           isOn: layer == 0 ? $profile.gamepadStickToDpad
                                            : $profile.touchStickToDpad)
                    VStack(alignment: .leading) {
                        let dz = layer == 0 ? profile.gamepadDeadzone : profile.touchDeadzone
                        Text("Stick deadzone: \(Int((dz * 100).rounded()))%")
                        Slider(value: layer == 0 ? $profile.gamepadDeadzone
                                                 : $profile.touchDeadzone,
                               in: 0...0.9, step: 0.05)
                    }
                    Button("Reset these buttons to normal") {
                        let identity = (0..<UInt32(16)).map { $0 }
                        if layer == 0 {
                            profile.gamepadMap = identity
                        } else {
                            profile.touchMap = identity
                        }
                    }
                }

                if profile.name != "Default" || !profile.game.isEmpty {
                    Section {
                        Button("Delete this profile", role: .destructive) { onDelete(profile) }
                    }
                }
            }
            .navigationTitle("Button mapping")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { onFinish(nil) }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { onFinish(profile) }
                }
            }
        }
    }

    private func binding(_ index: Int) -> Binding<UInt32> {
        Binding(
            get: {
                let map = layer == 0 ? profile.gamepadMap : profile.touchMap
                return index < map.count ? map[index] : UInt32(index)
            },
            set: { value in
                if layer == 0 {
                    while profile.gamepadMap.count < 16 {
                        profile.gamepadMap.append(UInt32(profile.gamepadMap.count))
                    }
                    profile.gamepadMap[index] = value
                } else {
                    while profile.touchMap.count < 16 {
                        profile.touchMap.append(UInt32(profile.touchMap.count))
                    }
                    profile.touchMap[index] = value
                }
            }
        )
    }
}

// MARK: - Settings card

/// The CONTROLLERS, MAPPING AND MOTION card in Settings.
struct InputSettingsSection: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var extras: InputExtras

    var body: some View {
        SettingsSection(title: "CONTROLLERS, MAPPING AND MOTION") {
            SettingsButton(title: "Controllers, button mapping and motion", role: .normal) {
                host.showControllers()
            }
            Toggle(isOn: $extras.motionEnabled) {
                Text("Tilt and motion sensor")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .tint(ShellPalette.accent)
            SettingsNote(
                "Tilt games (Yoshi Topsy-Turvy, WarioWare: Twisted!) read the phone's motion "
                + "sensor. It only runs while a game asks for it. Calibrate in Controllers by "
                + "holding the phone the way you play."
            )
            SettingsReadout(label: "Motion", value: extras.motionLine)
        }
        .sheet(isPresented: $extras.showingControllers) {
            ControllersScreen(host: host, extras: extras, controllers: host.controllers,
                              startOnBinding: extras.openOnBinding) {
                extras.showingControllers = false
            }
        }
    }
}
