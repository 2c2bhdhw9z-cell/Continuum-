// Continuum - where the two screens go, the TV, and the touch screen as a mouse.
//
// THIN ON PURPOSE. The engine owns every rectangle: the six DS/3DS layouts, the swap, which skin
// hole gets which picture, where a tap lands in the guest, what the TV shows and what stays on the
// phone, and the mouse's relative motion. See crates/emulator-bridge/src/gfx/screen_layout.rs and
// input/mod.rs. This file stores the user's choices, per system where the brief asks for that, and
// hands them over. Android will store the same choices and call the same engine.
//
// The external display is the one piece that has to be platform code: a second window on the TV's
// scene, with a CAMetalLayer the engine can retarget to. The engine still creates the only
// MTLDevice; the TV's layer gets a surface on the renderer's own instance and device.

import Foundation
import QuartzCore
import SwiftUI
import UIKit

// MARK: - The choices

/// DS / 3DS layout, swap and TV choices, and trackpad mode, remembered between launches.
@MainActor
final class ScreenModes: ObservableObject {

    /// The engine's six layouts, named for a settings screen.
    enum Layout: String, CaseIterable, Identifiable {
        case stacked
        case sideBySide
        case bigTop
        case bigBottom
        case topOnly
        case bottomOnly

        var id: String { rawValue }

        var label: String {
            switch self {
            case .stacked: return "Stacked"
            case .sideBySide: return "Side by side"
            case .bigTop: return "Big top, small bottom"
            case .bigBottom: return "Big bottom, small top"
            case .topOnly: return "Top screen only"
            case .bottomOnly: return "Bottom screen only"
            }
        }

        var engineValue: ScreenLayoutOption {
            switch self {
            case .stacked: return .stacked
            case .sideBySide: return .sideBySide
            case .bigTop: return .bigTop
            case .bigBottom: return .bigBottom
            case .topOnly: return .topOnly
            case .bottomOnly: return .bottomOnly
            }
        }
    }

    /// Systems with two screens. Matches `DualScreenGeometry::for_system` in the engine.
    static let dualScreenSystems: [GameSystem] = [.ds, .n3ds]

    @Published private(set) var layouts: [String: Layout] = [:]
    @Published private(set) var swapped: [String: Bool] = [:]
    @Published private(set) var trackpadSystems: Set<String> = []

    /// Show the game on a TV (AirPlay or a cable) when one is connected.
    @Published var useTV: Bool = true {
        didSet {
            guard oldValue != useTV else { return }
            UserDefaults.standard.set(useTV, forKey: Self.useTVKey)
            ExternalDisplayHub.shared.settingChanged()
        }
    }

    /// With a TV connected, keep the DS / 3DS touch screen on the phone.
    @Published var touchOnPhone: Bool = true {
        didSet {
            guard oldValue != touchOnPhone else { return }
            UserDefaults.standard.set(touchOnPhone, forKey: Self.touchOnPhoneKey)
            pushCurrent()
        }
    }

    /// Wider than tall. The big + small layouts go side by side when it is.
    private(set) var landscape = false

    /// The system whose settings are in the engine right now.
    private var current: GameSystem?

    /// Called after anything that moves a screen, so the host can re-ask the engine where the
    /// picture and the touch screen are.
    var onLayoutChanged: (() -> Void)?

    private let engine: ContinuumEngine

    private static let layoutsKey = "continuum.screens.layouts.v1"
    private static let swappedKey = "continuum.screens.swapped.v1"
    private static let trackpadKey = "continuum.input.trackpad.v1"
    private static let useTVKey = "continuum.display.useTV.v1"
    private static let touchOnPhoneKey = "continuum.display.touchOnPhone.v1"

    init(engine: ContinuumEngine) {
        self.engine = engine
        let defaults = UserDefaults.standard
        if let raw = defaults.dictionary(forKey: Self.layoutsKey) as? [String: String] {
            layouts = raw.compactMapValues { Layout(rawValue: $0) }
        }
        if let raw = defaults.dictionary(forKey: Self.swappedKey) as? [String: Bool] {
            swapped = raw
        }
        trackpadSystems = Set(defaults.stringArray(forKey: Self.trackpadKey) ?? [])
        if let stored = defaults.object(forKey: Self.useTVKey) as? Bool {
            useTV = stored
        }
        if let stored = defaults.object(forKey: Self.touchOnPhoneKey) as? Bool {
            touchOnPhone = stored
        }
    }

    // MARK: Layout and swap

    func layout(for system: GameSystem) -> Layout {
        layouts[system.rawValue] ?? .stacked
    }

    func setLayout(_ layout: Layout, for system: GameSystem) {
        guard self.layout(for: system) != layout else { return }
        layouts[system.rawValue] = layout
        UserDefaults.standard.set(layouts.mapValues(\.rawValue), forKey: Self.layoutsKey)
        if current == system {
            pushCurrent()
        }
    }

    func isSwapped(_ system: GameSystem) -> Bool {
        swapped[system.rawValue] ?? false
    }

    /// The one-tap swap for the running system. Returns the HUD line.
    func toggleSwap(for system: GameSystem) -> String {
        let now = !isSwapped(system)
        swapped[system.rawValue] = now
        UserDefaults.standard.set(swapped, forKey: Self.swappedKey)
        current = system
        pushCurrent()
        let order = now ? "the touch screen is first" : "back to the top screen first"
        if engine.skinCanSwap() {
            return "screens swapped: the skin's holes traded pictures, \(order)"
        }
        return "screens swapped: \(order) (\(layout(for: system).label.lowercased()))"
    }

    /// Pushes this system's choices into the engine. Call before a launch.
    func apply(for system: GameSystem?) {
        current = system
        pushCurrent()
    }

    /// The window turned. Only the big + small layouts care.
    func setLandscape(_ landscape: Bool) {
        guard self.landscape != landscape else { return }
        self.landscape = landscape
        pushCurrent()
    }

    private func pushCurrent() {
        let system = current
        let layout = system.map { self.layout(for: $0) } ?? .stacked
        let swap = system.map { isSwapped($0) } ?? false
        engine.setDualScreenSettings(settings: DualScreenSettings(
            layout: layout.engineValue,
            swapped: swap,
            landscape: landscape,
            touchOnPhone: touchOnPhone
        ))
        onLayoutChanged?()
    }

    // MARK: Trackpad

    func trackpad(for system: GameSystem) -> Bool {
        trackpadSystems.contains(system.rawValue)
    }

    func setTrackpad(_ on: Bool, for system: GameSystem) {
        guard trackpad(for: system) != on else { return }
        if on {
            trackpadSystems.insert(system.rawValue)
        } else {
            trackpadSystems.remove(system.rawValue)
        }
        UserDefaults.standard.set(Array(trackpadSystems).sorted(), forKey: Self.trackpadKey)
    }
}

// MARK: - The external display

/// The one place that knows whether a TV is connected and whether the engine is drawing to it.
///
/// A singleton because the TV's scene delegate is created by UIKit from the Info.plist entry and
/// has no path to `EngineHost`; the host registers itself here when its surface attaches.
@MainActor
final class ExternalDisplayHub {
    static let shared = ExternalDisplayHub()

    private weak var engine: ContinuumEngine?
    /// Where HUD lines go. Every connect, disconnect and failure writes one.
    private var report: ((String) -> Void)?
    private var wanted: () -> Bool = { true }
    /// The engine's picture moved (to the TV or back): re-ask where things are.
    private var onRetarget: (() -> Void)?

    /// The TV's view, while a TV scene is connected.
    private weak var view: ExternalMetalView?
    private var screenName = "TV"
    /// True while the engine is drawing to the TV.
    private(set) var attached = false

    private init() {}

    /// Called once the phone's surface is attached, which is the first moment the engine has a
    /// renderer to retarget. A TV connected before that is attached now.
    func bind(engine: ContinuumEngine,
              wanted: @escaping () -> Bool,
              report: @escaping (String) -> Void,
              onRetarget: @escaping () -> Void) {
        self.engine = engine
        self.wanted = wanted
        self.report = report
        self.onRetarget = onRetarget
        if view != nil {
            attachIfWanted()
        }
    }

    func connected(_ view: ExternalMetalView, name: String) {
        self.view = view
        screenName = name
        guard engine != nil else {
            // Reported, and retried by `bind` once the phone's surface is up.
            report?("TV connected (\(name)); waiting for the game view before moving the picture")
            return
        }
        attachIfWanted()
    }

    /// The TV went away. Detach FIRST, before UIKit releases the window and its layer, so the
    /// engine never presents into a layer that no longer exists.
    func disconnected() {
        let wasAttached = attached
        if wasAttached {
            _ = engine?.detachExternalDisplay()
        }
        attached = false
        view = nil
        report?(wasAttached
                ? "TV disconnected: the game is back on the phone"
                : "TV disconnected")
        onRetarget?()
    }

    /// The TV's drawable changed size.
    func resized(width: UInt32, height: UInt32) {
        guard attached, let engine else { return }
        engine.resizeExternalDisplay(width: width, height: height)
    }

    /// "Use TV when connected" changed.
    func settingChanged() {
        guard view != nil else { return }
        if wanted() {
            attachIfWanted()
        } else if attached {
            _ = engine?.detachExternalDisplay()
            attached = false
            view?.showMessage("Use TV when connected is off in Settings")
            report?("TV output switched off: the game is back on the phone")
            onRetarget?()
        }
    }

    private func attachIfWanted() {
        guard let view, let engine, !attached else { return }
        guard wanted() else {
            view.showMessage("Use TV when connected is off in Settings")
            report?("TV connected (\(screenName)), but Use TV when connected is off in Settings")
            return
        }
        let size = view.drawablePixels
        guard size.width > 0, size.height > 0 else {
            report?("TV connected (\(screenName)), but its screen reported no size yet; "
                    + "it will be used on the next layout")
            view.attachWhenSized = true
            return
        }
        let layer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(view.metalLayer).toOpaque()))
        do {
            let line = try engine.attachExternalDisplay(layer: layer,
                                                        width: UInt32(size.width),
                                                        height: UInt32(size.height))
            attached = true
            view.showMessage(nil)
            report?("\(line) on \(screenName)")
            onRetarget?()
        } catch {
            view.showMessage("Continuum could not draw here: \(error)")
            report?("TV connected (\(screenName)), but the picture could not move to it: \(error)")
        }
    }

    /// The view finally has a size after a connect that had none.
    fileprivate func viewBecameSized() {
        attachIfWanted()
    }
}

/// The TV's view: a CAMetalLayer the engine draws into, and a plain line when it does not.
final class ExternalMetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    /// Set when a connect arrived before the screen had a size.
    var attachWhenSized = false

    private let label = UILabel()

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .black
        metalLayer.presentsWithTransaction = false
        label.textColor = UIColor(white: 1, alpha: 0.7)
        label.font = .monospacedSystemFont(ofSize: 22, weight: .regular)
        label.numberOfLines = 0
        label.textAlignment = .center
        label.isHidden = true
        addSubview(label)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    /// Device pixels, like the phone's canvas.
    var drawablePixels: CGSize {
        let scale = window?.screen.nativeScale ?? window?.windowScene?.screen.nativeScale ?? 1
        return CGSize(width: (bounds.width * scale).rounded(), height: (bounds.height * scale).rounded())
    }

    func showMessage(_ text: String?) {
        label.text = text
        label.isHidden = text == nil
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        label.frame = bounds.insetBy(dx: 40, dy: 40)
        let scale = window?.screen.nativeScale ?? window?.windowScene?.screen.nativeScale ?? 1
        metalLayer.contentsScale = scale
        let size = drawablePixels
        guard size.width > 0, size.height > 0 else { return }
        if metalLayer.drawableSize != size {
            metalLayer.drawableSize = size
        }
        // UIView is main-actor isolated, so this is already on the hub's actor.
        if attachWhenSized {
            attachWhenSized = false
            ExternalDisplayHub.shared.viewBecameSized()
        } else {
            ExternalDisplayHub.shared.resized(width: UInt32(size.width), height: UInt32(size.height))
        }
    }
}

/// The TV's scene. Named in Info.plist (`UIApplicationSceneManifest`, role
/// `UIWindowSceneSessionRoleExternalDisplayNonInteractive`), so UIKit creates it whenever a TV
/// connects: AirPlay screen mirroring and a cable both arrive this way. The app's own scene stays
/// SwiftUI's.
@objc(ExternalDisplaySceneDelegate)
final class ExternalDisplaySceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene,
               willConnectTo session: UISceneSession,
               options connectionOptions: UIScene.ConnectionOptions) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        let controller = UIViewController()
        let view = ExternalMetalView(frame: windowScene.coordinateSpace.bounds)
        controller.view = view
        window.rootViewController = controller
        window.isHidden = false
        self.window = window
        view.layoutIfNeeded()
        let name = windowScene.screen.bounds.size == .zero
            ? "TV"
            : "TV \(Int(windowScene.screen.nativeBounds.width))x\(Int(windowScene.screen.nativeBounds.height))"
        ExternalDisplayHub.shared.connected(view, name: name)
    }

    func sceneDidDisconnect(_ scene: UIScene) {
        ExternalDisplayHub.shared.disconnected()
        window?.isHidden = true
        window = nil
    }
}

// MARK: - Settings

/// The TWO SCREENS AND TV section, and trackpad mode.
struct ScreenModesSettingsSection: View {
    @ObservedObject var modes: ScreenModes

    var body: some View {
        SettingsSection(title: "TWO SCREENS, TV AND MOUSE") {
            ForEach(ScreenModes.dualScreenSystems, id: \.self) { system in
                SettingsLabel("\(system.displayName) layout")
                Picker(system.displayName, selection: Binding(
                    get: { modes.layout(for: system) },
                    set: { modes.setLayout($0, for: system) }
                )) {
                    ForEach(ScreenModes.Layout.allCases) { layout in
                        Text(layout.label).tag(layout)
                    }
                }
                .pickerStyle(.menu)
                .tint(ShellPalette.accent)
            }
            SettingsNote(
                "Each system remembers its own layout. The swap button in the player exchanges "
                + "which screen goes where, and with an imported skin it trades the pictures in "
                + "the skin's two holes. The touch screen follows the bottom screen wherever it is "
                + "drawn."
            )

            Toggle(isOn: $modes.useTV) {
                Text("Use TV when connected")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .tint(ShellPalette.accent)
            Toggle(isOn: $modes.touchOnPhone) {
                Text("Keep the DS and 3DS touch screen on the phone")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .tint(ShellPalette.accent)
            SettingsNote(
                "With AirPlay or a cable, the game goes to the TV and the controls stay on the "
                + "phone. On a DS or 3DS the TV shows the top screen and the phone keeps the touch "
                + "screen, unless the second switch is off, in which case the TV shows the whole "
                + "layout."
            )

            SettingsLabel("Touch screen as a mouse")
            ForEach(GameSystem.allCases.filter { !ScreenModes.dualScreenSystems.contains($0) },
                    id: \.self) { system in
                Toggle(isOn: Binding(
                    get: { modes.trackpad(for: system) },
                    set: { modes.setTrackpad($0, for: system) }
                )) {
                    Text(system.displayName)
                        .font(.system(size: 14))
                        .foregroundStyle(.white)
                }
                .tint(ShellPalette.accent)
            }
            SettingsNote(
                "Drag on the picture to move the mouse. Tap to left click, tap with two fingers "
                + "to right click, press and hold then drag to hold the button down. Only games "
                + "that support a mouse respond, such as Mario Paint on the Super Nintendo or "
                + "PlayStation mouse games. Takes effect the next time a game of that system starts."
            )
        }
    }
}
