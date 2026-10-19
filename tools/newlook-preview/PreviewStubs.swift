// Stand-ins for the two app types NewLookMotion.swift reads, so the REAL motion code can be
// compiled on its own into a tiny simulator app and filmed in CI (.github/workflows/
// newlook-preview.yml). The full app cannot be built for the simulator: its Rust engine, C++
// core wrapper and libsmb2 are built for iPhone hardware only.
//
// The values match the app's defaults: gold accent gradient on charcoal.

import SwiftUI

final class NewLookColors: ObservableObject {
    static let shared = NewLookColors()

    private static func rgb(_ hex: UInt32) -> SIMD3<Double> {
        SIMD3(Double((hex >> 16) & 0xFF) / 255, Double((hex >> 8) & 0xFF) / 255, Double(hex & 0xFF) / 255)
    }
    private static func color(_ v: SIMD3<Double>) -> Color { Color(red: v.x, green: v.y, blue: v.z) }

    // FFD66B-F2B233 (the default accent) and the neighbours the app derives from it.
    var motionRGB: [SIMD3<Double>] { [0xFFD66B, 0xC6E85A, 0xF2B233, 0xFF7A4D].map(Self.rgb) }
    var motionColors: [Color] { motionRGB.map(Self.color) }
    var accentStops: [Color] { [Self.color(Self.rgb(0xFFD66B)), Self.color(Self.rgb(0xF2B233))] }
    var accent: Color { Self.color(Self.rgb(0xF8C44F)) }
    var sparkColor: Color { accent }
    var accentFill: LinearGradient {
        LinearGradient(colors: accentStops, startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

enum ShellPalette {
    static var accentFill: AnyShapeStyle { AnyShapeStyle(NewLookColors.shared.accentFill) }
    static var gold: Color { NewLookColors.shared.accent }
    static let charcoal = Color(red: 0x13 / 255, green: 0x13 / 255, blue: 0x16 / 255)
}
