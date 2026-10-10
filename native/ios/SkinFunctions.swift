// Continuum - every Manic EMU custom function button, in ONE dispatcher.
//
// A skin item whose `inputs` names a function (`quickSave`, `reverseScreens`, `palette`...) and an
// extra floating button set to an app action both end up in `EngineHost.performSkinFunction`. There
// is no second table anywhere: `PadAppAction` (the floating buttons' list) is this list plus the
// floating-only `menu`, and its handler forwards here.
//
// The top half of this file is PURE FOUNDATION: the list, the names Manic and Delta use for each,
// what each one is wired to, and which ones are held rather than tapped. It compiles with real
// `swiftc` on Linux so the table can be checked by a program. The bottom half, behind
// `canImport(UIKit)`, is the dispatcher itself and the small amount of shared player state the
// functions need (hidden controls, orientation lock, which sheet to open).
//
// Functions other workers are building call EngineHost methods that do not exist on this tree
// yet. Their temporary stand-ins are all in SkinFunctionPending.swift, one per method, each saying
// plainly that the feature is not in this build. When the real method merges, its stub is deleted
// and nothing here changes.

import Foundation

// MARK: - The list

/// Every Manic EMU custom function input, spelled exactly as Manic spells it (including
/// `toggleControlls`, which is Manic's own spelling).
enum SkinFunction: String, CaseIterable, Codable, Sendable {
    case flex
    case quickSave
    case quickLoad
    case fastForward
    case toggleFastForward
    case fastForward2x
    case fastForward3x
    case fastForward4x
    case reverseScreens
    case volume
    case saveStates
    case cheatCodes
    case skins
    case filters
    case screenshot
    case haptics
    case controllers
    case orientation
    case functionLayout
    case restart
    case resolution
    case quit
    case amiibo
    case homeMenu
    case toggleControlls
    case blowing
    case palette
    case swapDisk
    case insertDisc
    case shake
    case toggleAnalog
    case retroAchievements
    case airPlayScaling
    case airPlayLayout
    case gameplayManuals
    case triggerPro
    case tvType
    case leftDifficulty
    case rightDifficulty
    case screenScaling
    case j2meSettings
    case dosSettings
    case coreSettings
    case rewind
    case slowMotion
    case wswanRotation
    case ndsLidToggle
    case skinButtonBinding

    /// The function a skin's input name means, or nil when it is a game button (or nothing).
    /// Case, spaces, dashes and underscores are ignored, and a few spellings other skins use are
    /// accepted beside Manic's own.
    static func named(_ raw: String) -> SkinFunction? {
        let key = canonical(raw)
        guard !key.isEmpty else { return nil }
        if let hit = byCanonical[key] { return hit }
        return aliases[key]
    }

    static func canonical(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .replacingOccurrences(of: " ", with: "")
            .replacingOccurrences(of: "_", with: "")
            .replacingOccurrences(of: "-", with: "")
    }

    private static let byCanonical: [String: SkinFunction] = {
        var out: [String: SkinFunction] = [:]
        for function in SkinFunction.allCases { out[canonical(function.rawValue)] = function }
        return out
    }()

    /// Other spellings seen in skins. Never a game button name: `home` and `menu` stay buttons
    /// (the 3DS Home button is a real input), and Delta's `menu` is handled by the importer.
    static let aliases: [String: SkinFunction] = [
        "togglecontrols": .toggleControlls,
        "hidecontrols": .toggleControlls,
        "swapdisc": .swapDisk,
        "changedisc": .swapDisk,
        "insertdisk": .insertDisc,
        "reversescreen": .reverseScreens,
        "swapscreens": .reverseScreens,
        "mute": .volume,
        "savestate": .saveStates,
        "cheats": .cheatCodes,
        "cheat": .cheatCodes,
        "achievements": .retroAchievements,
        "reset": .restart,
        "fastforwardtoggle": .toggleFastForward,
        "slowmo": .slowMotion,
        "lid": .ndsLidToggle,
        "ndslid": .ndsLidToggle,
        "rotation": .wswanRotation,
        "manual": .gameplayManuals,
        "manuals": .gameplayManuals,
        "blow": .blowing,
        "buttonbinding": .skinButtonBinding,
    ]

    /// Held functions act for as long as the finger is down. The rest act once, on the press.
    var isHold: Bool {
        switch self {
        case .fastForward, .fastForward2x, .fastForward3x, .fastForward4x, .rewind, .blowing:
            return true
        default:
            return false
        }
    }

    /// Multipliers for the three fixed-speed fast-forward buttons.
    var holdSpeed: Double? {
        switch self {
        case .fastForward2x: return 2
        case .fastForward3x: return 3
        case .fastForward4x: return 4
        default: return nil
        }
    }

    /// The real state a switch button bound to this function shows when the skin loads.
    /// Nil for functions that have no on/off state.
    var boundState: SkinFunctionState? {
        switch self {
        case .reverseScreens: return .screensSwapped
        case .volume: return .muted
        case .toggleControlls: return .controlsHidden
        case .toggleAnalog: return .analogMode
        case .tvType: return .tvColour
        case .leftDifficulty: return .leftDifficultyA
        case .rightDifficulty: return .rightDifficultyA
        default: return nil
        }
    }

    var title: String {
        switch self {
        case .flex: return "All functions menu"
        case .quickSave: return "Quick save"
        case .quickLoad: return "Quick load"
        case .fastForward: return "Fast forward (hold)"
        case .toggleFastForward: return "Fast forward on/off"
        case .fastForward2x: return "Fast forward 2x (hold)"
        case .fastForward3x: return "Fast forward 3x (hold)"
        case .fastForward4x: return "Fast forward 4x (hold)"
        case .reverseScreens: return "Swap screens"
        case .volume: return "Sound on/off"
        case .saveStates: return "Save slots"
        case .cheatCodes: return "Cheats"
        case .skins: return "Change skin"
        case .filters: return "Filters"
        case .screenshot: return "Screenshot"
        case .haptics: return "Haptics"
        case .controllers: return "Controllers"
        case .orientation: return "Lock orientation"
        case .functionLayout: return "Edit layout"
        case .restart: return "Restart game"
        case .resolution: return "Resolution"
        case .quit: return "Quit game"
        case .amiibo: return "Amiibo"
        case .homeMenu: return "3DS Home"
        case .toggleControlls: return "Hide/show controls"
        case .blowing: return "Blow into mic (hold)"
        case .palette: return "Palette"
        case .swapDisk: return "Swap disc"
        case .insertDisc: return "Insert disc"
        case .shake: return "Shake"
        case .toggleAnalog: return "Analog mode"
        case .retroAchievements: return "Achievements"
        case .airPlayScaling: return "AirPlay scaling"
        case .airPlayLayout: return "AirPlay layout"
        case .gameplayManuals: return "Game manual"
        case .triggerPro: return "Button profile"
        case .tvType: return "TV type (color/BW)"
        case .leftDifficulty: return "Left difficulty"
        case .rightDifficulty: return "Right difficulty"
        case .screenScaling: return "Screen scaling"
        case .j2meSettings: return "J2ME settings"
        case .dosSettings: return "DOS settings"
        case .coreSettings: return "Core settings"
        case .rewind: return "Rewind (hold)"
        case .slowMotion: return "Slow motion"
        case .wswanRotation: return "Rotate screen"
        case .ndsLidToggle: return "Close/open DS lid"
        case .skinButtonBinding: return "Button binding"
        }
    }

    /// What a small round button says. Short on purpose.
    var caption: String {
        switch self {
        case .flex: return "MENU"
        case .quickSave: return "SAVE"
        case .quickLoad: return "LOAD"
        case .fastForward: return "FF"
        case .toggleFastForward: return "FF\u{00B7}T"
        case .fastForward2x: return "2X"
        case .fastForward3x: return "3X"
        case .fastForward4x: return "4X"
        case .reverseScreens: return "SWAP"
        case .volume: return "MUTE"
        case .saveStates: return "SLOTS"
        case .cheatCodes: return "CHEAT"
        case .skins: return "SKIN"
        case .filters: return "FILTR"
        case .screenshot: return "SHOT"
        case .haptics: return "HAPT"
        case .controllers: return "PADS"
        case .orientation: return "ROT"
        case .functionLayout: return "EDIT"
        case .restart: return "RESET"
        case .resolution: return "RES"
        case .quit: return "QUIT"
        case .amiibo: return "AMIIB"
        case .homeMenu: return "HOME"
        case .toggleControlls: return "HIDE"
        case .blowing: return "BLOW"
        case .palette: return "PAL"
        case .swapDisk: return "DISC"
        case .insertDisc: return "INS"
        case .shake: return "SHAKE"
        case .toggleAnalog: return "ANLG"
        case .retroAchievements: return "RA"
        case .airPlayScaling: return "TV\u{00B7}S"
        case .airPlayLayout: return "TV\u{00B7}L"
        case .gameplayManuals: return "BOOK"
        case .triggerPro: return "PRO"
        case .tvType: return "TV"
        case .leftDifficulty: return "L\u{00B7}DIF"
        case .rightDifficulty: return "R\u{00B7}DIF"
        case .screenScaling: return "SCALE"
        case .j2meSettings: return "J2ME"
        case .dosSettings: return "DOS"
        case .coreSettings: return "CORE"
        case .rewind: return "REW"
        case .slowMotion: return "SLOW"
        case .wswanRotation: return "ROT"
        case .ndsLidToggle: return "LID"
        case .skinButtonBinding: return "BIND"
        }
    }

    /// What the function is wired to. `pending` names an EngineHost method another worker owns;
    /// until it merges, SkinFunctionPending.swift answers it with a plain "not in this build" line.
    var route: SkinFunctionRoute {
        switch self {
        case .flex: return .sheet("functions")
        case .quickSave: return .existing("saveStates.saveToNewSlot")
        case .quickLoad: return .existing("saveStates.load(newest)")
        case .fastForward: return .existing("emulation.beginFastForward/endFastForward")
        case .toggleFastForward: return .existing("emulation.beginFastForward/endFastForward")
        case .fastForward2x, .fastForward3x, .fastForward4x: return .pending("setHoldSpeed")
        case .reverseScreens: return .existing("swapScreens")
        case .volume: return .existing("emulation.muted")
        case .saveStates: return .sheet("saveSlots")
        case .cheatCodes: return .sheet("cheats")
        case .skins: return .sheet("skins")
        case .filters: return .pending("showFilters")
        case .screenshot: return .existing("captureScreenshot")
        case .haptics: return .sheet("haptics")
        case .controllers: return .pending("showControllers")
        case .orientation: return .existing("SkinRuntime.toggleOrientationLock")
        case .functionLayout: return .sheet("layoutEditor")
        case .restart: return .existing("resetGame")
        case .resolution: return .pending("cycleResolution")
        case .quit: return .existing("leavePlayer")
        case .amiibo: return .sheet("amiibo")
        case .homeMenu: return .pending("pressHomeButton")
        case .toggleControlls: return .existing("SkinRuntime.controlsHidden")
        case .blowing: return .pending("blowIntoMic")
        case .palette: return .pending("cyclePalette")
        case .swapDisk: return .pending("swapDisc")
        case .insertDisc: return .pending("insertDisc")
        case .shake: return .pending("shake")
        case .toggleAnalog: return .pending("toggleAnalogMode")
        case .retroAchievements: return .sheet("achievements")
        case .airPlayScaling: return .pending("cycleAirPlayScaling")
        case .airPlayLayout: return .pending("cycleAirPlayLayout")
        case .gameplayManuals: return .pending("showGameplayManual")
        case .triggerPro: return .pending("cycleTriggerProfile")
        case .tvType: return .pending("toggleTVType")
        case .leftDifficulty, .rightDifficulty: return .pending("toggleDifficulty")
        case .screenScaling: return .pending("cycleScreenScaling")
        case .j2meSettings: return .existing("showJ2MESettings")
        case .dosSettings, .coreSettings: return .pending("showCoreSettings")
        case .rewind: return .existing("emulation.beginRewind/endRewind")
        case .slowMotion: return .pending("toggleSlowMotion")
        case .wswanRotation: return .pending("rotateScreen")
        case .ndsLidToggle: return .pending("toggleDSLid")
        case .skinButtonBinding: return .pending("showButtonBinding")
        }
    }

    /// Refused during online play, because acting on one phone and not the other splits the two
    /// games apart. The engine refuses these too; this says so on the status line first.
    var refusedOnline: Bool {
        switch self {
        case .quickLoad, .fastForward, .toggleFastForward, .fastForward2x, .fastForward3x,
             .fastForward4x, .restart, .rewind, .slowMotion:
            return true
        default:
            return false
        }
    }
}

/// Where a function goes.
enum SkinFunctionRoute: Equatable, Sendable {
    /// Code already in the app, named for the reader.
    case existing(String)
    /// A sheet or cover the player screen opens.
    case sheet(String)
    /// An EngineHost method another worker provides (see SkinFunctionPending.swift).
    case pending(String)
}

/// The on/off states a switch button can be bound to.
enum SkinFunctionState: String, CaseIterable, Sendable {
    case screensSwapped
    case muted
    case controlsHidden
    case analogMode
    case tvColour
    case leftDifficultyA
    case rightDifficultyA

    /// The EngineHost reader, when the state comes from another worker's method.
    var pendingReader: String? {
        switch self {
        case .analogMode: return "isAnalogMode"
        case .tvColour: return "currentTVTypeIsColor"
        case .leftDifficultyA, .rightDifficultyA: return "difficultyIsA"
        default: return nil
        }
    }
}

/// Every EngineHost method this file calls that another worker provides. The check program
/// proves each one is either a route above, a state reader, or used by the functions menu, and
/// that SkinFunctionPending.swift stubs every one of them.
enum SkinFunctionPendingNames {
    static let all: [String] = [
        "showCoreSettings", "showFilters", "cyclePalette", "cycleFastForward", "setHoldSpeed",
        "toggleSlowMotion", "swapDisc", "insertDisc", "cycleResolution", "cycleScreenScaling",
        "cycleAirPlayScaling", "cycleAirPlayLayout", "rotateScreen", "toggleTVType",
        "toggleDifficulty", "currentTVTypeIsColor", "difficultyIsA", "shake", "toggleAnalogMode",
        "isAnalogMode", "toggleDSLid", "blowIntoMic", "pressHomeButton", "showControllers",
        "cycleTriggerProfile", "showButtonBinding", "showGameplayManual",
    ]
    /// Reached only from the all-functions menu (`flex`), not from a single function name.
    static let menuOnly: [String] = ["cycleFastForward"]
}

// MARK: - The floating buttons' list

/// Something an extra floating button can do that is not a game input: every skin function, plus
/// `menu` (pause), which floating buttons had before the skin functions existed. Raw values are the
/// stored strings, so a button saved by an older build (`quickSave`, `rewind`, `menu`...) still
/// decodes to the same action.
enum PadAppAction: String, Codable, CaseIterable, Identifiable, Sendable {
    case quickSave
    case quickLoad
    case fastForward
    case rewind
    case screenshot
    case menu
    case flex
    case toggleFastForward
    case fastForward2x
    case fastForward3x
    case fastForward4x
    case reverseScreens
    case volume
    case saveStates
    case cheatCodes
    case skins
    case filters
    case haptics
    case controllers
    case orientation
    case functionLayout
    case restart
    case resolution
    case quit
    case amiibo
    case homeMenu
    case toggleControlls
    case blowing
    case palette
    case swapDisk
    case insertDisc
    case shake
    case toggleAnalog
    case retroAchievements
    case airPlayScaling
    case airPlayLayout
    case gameplayManuals
    case triggerPro
    case tvType
    case leftDifficulty
    case rightDifficulty
    case screenScaling
    case j2meSettings
    case dosSettings
    case coreSettings
    case slowMotion
    case wswanRotation
    case ndsLidToggle
    case skinButtonBinding

    var id: String { rawValue }

    /// The skin function this action is. Nil only for `menu`.
    var skinFunction: SkinFunction? { SkinFunction(rawValue: rawValue) }

    var title: String {
        if self == .menu { return "Menu (pause)" }
        return skinFunction?.title ?? rawValue
    }

    /// What the circle says. Short, because the button may be small.
    var caption: String {
        if self == .menu { return "MENU" }
        return skinFunction?.caption ?? "?"
    }

    /// Held actions act for as long as the finger is down; the rest act once, on the press.
    var isHold: Bool {
        skinFunction?.isHold ?? false
    }
}

#if canImport(UIKit)
import SwiftUI
import UIKit

// MARK: - Shared player state the functions need

/// Which sheet the player should open for a function. `Identifiable` for `.sheet(item:)`.
enum SkinFunctionSheet: String, Identifiable {
    case functions
    case saveSlots
    case cheats
    case skins
    case haptics
    case amiibo
    case achievements

    var id: String { rawValue }
}

/// State that lives beside the player rather than inside one view: hidden controls, the
/// orientation lock and the sheet a function asked for. One shared object, because the pad (UIKit),
/// the player screen (SwiftUI) and the app delegate all read it and none of them owns the others.
@MainActor
final class SkinRuntime: ObservableObject {
    static let shared = SkinRuntime()

    @Published var sheet: SkinFunctionSheet?
    @Published var showLayoutEditor = false
    /// True while `toggleControlls` has hidden the on-screen controls. They still work.
    @Published private(set) var controlsHidden = false
    /// Nil when the phone may rotate freely.
    @Published private(set) var orientationLock: UIInterfaceOrientationMask?

    /// Every skin button press plays the skin's `sound.caf` when this is on.
    @Published var buttonSoundEnabled: Bool {
        didSet { UserDefaults.standard.set(buttonSoundEnabled, forKey: Self.soundKey) }
    }

    private static let soundKey = "continuum.skins.buttonSound.v1"

    private init() {
        buttonSoundEnabled = (UserDefaults.standard.object(forKey: Self.soundKey) as? Bool) ?? true
    }

    func setControlsHidden(_ hidden: Bool) {
        guard controlsHidden != hidden else { return }
        controlsHidden = hidden
    }

    /// Locks to the orientation the phone is in now, or unlocks. Returns the status line.
    func toggleOrientationLock() -> String {
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
            ?? UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        if orientationLock != nil {
            orientationLock = nil
            refresh(scene: scene, mask: .all)
            return "orientation unlocked: the screen turns with the phone again"
        }
        let current = scene?.interfaceOrientation ?? .portrait
        let mask: UIInterfaceOrientationMask
        let word: String
        switch current {
        case .landscapeLeft: mask = .landscapeLeft; word = "landscape"
        case .landscapeRight: mask = .landscapeRight; word = "landscape"
        case .portraitUpsideDown: mask = .portraitUpsideDown; word = "portrait"
        default: mask = .portrait; word = "portrait"
        }
        orientationLock = mask
        refresh(scene: scene, mask: mask)
        return "orientation locked to \(word); press it again to unlock"
    }

    /// Clears what a game left behind, so the next game starts with visible controls.
    /// The orientation lock goes too: it is a lock for one game, not for the library.
    func gameEnded() {
        controlsHidden = false
        sheet = nil
        showLayoutEditor = false
        if orientationLock != nil {
            _ = toggleOrientationLock()
        }
    }

    private func refresh(scene: UIWindowScene?, mask: UIInterfaceOrientationMask) {
        guard let scene else { return }
        for window in scene.windows {
            window.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()
        }
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: mask)) { _ in }
    }
}

/// The app delegate, present only to answer which orientations are allowed while a lock is on.
/// If another part of the app needs an app delegate, add its methods here: SwiftUI allows one.
@MainActor
final class ContinuumAppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        SkinRuntime.shared.orientationLock ?? .all
    }
}

// MARK: - The dispatcher

extension EngineHost {
    /// Runs one skin or floating-button function. `pressed` is true on the press and false on
    /// the release; held functions act on both, the rest only on the press. Every branch leaves a
    /// plain line on the status strip, refusals included.
    func performSkinFunction(_ function: SkinFunction, pressed: Bool) {
        if !function.isHold && !pressed { return }
        guard running || webPlayer != nil, activeEntry != nil else {
            if pressed { status = "\(function.title): no game is running" }
            return
        }
        // A bundled player (Flash, J2ME) has no engine session behind it, so the functions that
        // need one say so here, plainly, before any arm below reaches for the engine. The rest
        // (quit, restart, screenshot, volume, hide controls, skins, the J2ME settings) run through
        // their arms like any game's. The table is WebPlayerKind.refusal in WebPlayerCore.swift.
        if let player = webPlayer, let refusal = player.kind.refusal(for: function) {
            if pressed { status = refusal }
            return
        }
        if function.refusedOnline && netplayLive {
            if pressed { status = "\(function.title) is off during online play" }
            return
        }
        let runtime = SkinRuntime.shared
        switch function {
        case .flex:
            runtime.sheet = .functions
        case .quickSave:
            saveStates.saveToNewSlot()
        case .quickLoad:
            guard let entry = activeEntry else { return }
            guard let newest = saveStates.states(for: entry).first else {
                status = "quick load: \(entry.name) has no saved state yet"
                return
            }
            // The store says why on the status line when a load is refused.
            _ = saveStates.load(newest)
        case .fastForward:
            if pressed { emulation.beginFastForward() } else { emulation.endFastForward() }
        case .toggleFastForward:
            if emulation.isFastForwarding {
                emulation.endFastForward()
                status = "fast forward off"
            } else {
                emulation.beginFastForward()
                status = "fast forward on at \(emulation.fastForward.label); press again to stop"
            }
        case .fastForward2x, .fastForward3x, .fastForward4x:
            status = setHoldSpeed(function.holdSpeed ?? 2, held: pressed)
        case .reverseScreens:
            swapScreens()
        case .volume:
            emulation.muted.toggle()
            status = emulation.muted ? "sound off" : "sound on"
        case .saveStates:
            runtime.sheet = .saveSlots
        case .cheatCodes:
            runtime.sheet = .cheats
        case .skins:
            runtime.sheet = .skins
        case .filters:
            status = showFilters()
        case .screenshot:
            if let entry = activeEntry { captureScreenshot(of: entry) }
        case .haptics:
            runtime.sheet = .haptics
        case .controllers:
            status = showControllers()
        case .orientation:
            status = runtime.toggleOrientationLock()
        case .functionLayout:
            runtime.showLayoutEditor = true
        case .restart:
            resetGame()
        case .resolution:
            status = cycleResolution()
        case .quit:
            leavePlayer()
        case .amiibo:
            guard activeSystem == .n3ds else {
                status = "Amiibo: only the 3DS reads Amiibo"
                return
            }
            runtime.sheet = .amiibo
        case .homeMenu:
            status = pressHomeButton()
        case .toggleControlls:
            runtime.setControlsHidden(!runtime.controlsHidden)
            padInput.view?.setNeedsLayout()
            status = runtime.controlsHidden
                ? "controls hidden; they still work, press the same place to show them"
                : "controls shown"
        case .blowing:
            status = blowIntoMic(held: pressed)
        case .palette:
            status = cyclePalette()
        case .swapDisk:
            status = swapDisc()
        case .insertDisc:
            status = insertDisc()
        case .shake:
            status = shake()
        case .toggleAnalog:
            status = toggleAnalogMode()
        case .retroAchievements:
            runtime.sheet = .achievements
        case .airPlayScaling:
            status = cycleAirPlayScaling()
        case .airPlayLayout:
            status = cycleAirPlayLayout()
        case .gameplayManuals:
            status = showGameplayManual()
        case .triggerPro:
            status = cycleTriggerProfile()
        case .tvType:
            status = toggleTVType()
        case .leftDifficulty:
            status = toggleDifficulty(left: true)
        case .rightDifficulty:
            status = toggleDifficulty(left: false)
        case .screenScaling:
            status = cycleScreenScaling()
        case .j2meSettings:
            status = showJ2MESettings()
        case .dosSettings, .coreSettings:
            status = showCoreSettings()
        case .rewind:
            if pressed {
                guard emulation.rewindEnabled else {
                    status = "rewind button: rewind is off in Settings, so there is nothing to rewind"
                    return
                }
                emulation.beginRewind()
            } else {
                emulation.endRewind()
            }
        case .slowMotion:
            status = toggleSlowMotion()
        case .wswanRotation:
            status = rotateScreen()
        case .ndsLidToggle:
            status = toggleDSLid()
        case .skinButtonBinding:
            status = showButtonBinding()
        }
    }

    /// Points the pad's skin function box, the switch-state reader and the button sound's volume
    /// at this host. Called once from `init`, beside `padInput.onAppAction`.
    func wireSkinFunctions() {
        padInput.onSkinFunction = { [weak self] function, pressed in
            self?.performSkinFunction(function, pressed: pressed)
        }
        padInput.skinFunctionState = { [weak self] function in
            self?.skinFunctionState(function)
        }
        let emulation = self.emulation
        SkinButtonSound.shared.volume = { [weak emulation] in
            guard let emulation, !emulation.muted else { return 0 }
            return Float(emulation.volume)
        }
    }

    /// A function named by a skin. Unknown names say so rather than doing nothing.
    func performSkinFunction(named name: String, pressed: Bool) {
        guard let function = SkinFunction.named(name) else {
            if pressed { status = "skin button: \"\(name)\" is not a function this app knows" }
            return
        }
        performSkinFunction(function, pressed: pressed)
    }

    /// The real state a bound switch shows. Nil when the function has no state.
    func skinFunctionState(_ function: SkinFunction) -> Bool? {
        guard let state = function.boundState else { return nil }
        switch state {
        case .screensSwapped:
            guard let system = activeSystem else { return false }
            return screenModes.isSwapped(system)
        case .muted:
            return emulation.muted
        case .controlsHidden:
            return SkinRuntime.shared.controlsHidden
        case .analogMode:
            return isAnalogMode()
        case .tvColour:
            return currentTVTypeIsColor()
        case .leftDifficultyA:
            return difficultyIsA(left: true)
        case .rightDifficultyA:
            return difficultyIsA(left: false)
        }
    }
}
#endif
