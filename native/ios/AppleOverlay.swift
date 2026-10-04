// AppleOverlay.swift
//
// Apple's Metal Performance HUD: the grey box of FPS / GPU / memory numbers iOS draws over the
// game when Developer Mode is on and Settings > Developer > Graphics HUD is enabled, or when the
// app's own defaults ask for it. It is not part of Continuum and it covers the picture, so there
// is a switch for it in Settings > Diagnostics, OFF by default.
//
// Two levers, because iOS reads the HUD request in two places:
//   - `CAMetalLayer.developerHUDProperties` on every layer the engine draws into. An empty
//     dictionary (no "mode" key) asks for the HUD to be hidden; "mode": "default" shows it. This
//     takes effect at once on the live layers.
//   - The `MetalForceHudEnabled` user default, which Metal reads when the app starts. That one
//     only takes effect on the next launch, hence the restart note on the switch.

import QuartzCore
import SwiftUI

enum AppleOverlay {
    static let key = "continuum.appleMetalHUD.v1"
    static let changed = Notification.Name("continuum.appleMetalHUD.changed")

    static var enabled: Bool { UserDefaults.standard.bool(forKey: key) }

    /// Called once at launch and whenever the switch moves.
    static func syncLaunchDefault() {
        UserDefaults.standard.set(enabled, forKey: "MetalForceHudEnabled")
    }

    static func apply(to layer: CAMetalLayer) {
        layer.developerHUDProperties = enabled ? ["mode": "default"] : [:]
    }
}

/// The switch, in Settings > Diagnostics.
struct AppleOverlayToggle: View {
    @AppStorage(AppleOverlay.key) private var enabled = false

    var body: some View {
        Toggle(isOn: $enabled) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Apple performance overlay")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                Text("Apple's FPS / GPU box drawn over the game. Off hides it. If it is still "
                     + "showing, close Continuum fully (swipe it away) and open it again.")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
        .tint(ShellPalette.accent)
        .onChange(of: enabled) { _ in
            AppleOverlay.syncLaunchDefault()
            NotificationCenter.default.post(name: AppleOverlay.changed, object: nil)
        }
    }
}
