// Continuum - the New look's colours (Settings, LOOK, with New on).
//
// Its own file, with nothing in it but SwiftUI and UIKit, so the simulator preview
// (tools/newlook-preview) compiles the REAL colours rather than a copy of them.

import SwiftUI
import UIKit

// MARK: - The New look's colours and covers (Settings, LOOK)

/// Everything the New look's colour panels under Settings, LOOK set (NewLookTheme.swift): the accent (one colour or a
/// gradient of two), saved colours, the glow round the covers, the ring next to CONTINUUM (and
/// whether it spins), the page's background (and whether it moves), and the covers' corners,
/// square covers and hidden titles.
///
/// Stored in UserDefaults, nil meaning the default, so Reset just forgets them. A colour is a
/// hex string, "FAC442", or two joined by a dash for a gradient, "FFD66B-F2B233". Deliberately
/// NOT main-actor isolated, for the reason `LookStore` is not: `ShellPalette` reads it from static
/// colour properties. `version` changes with every pick; `LibraryShell` uses it to redraw
/// Home, Library, Favorites and the tab bar, while Settings redraws in place and keeps its scroll.
final class NewLookColors: ObservableObject {
    static let shared = NewLookColors()

    enum GlowMode: String, CaseIterable, Identifiable {
        case console, accent, off, custom
        var id: String { rawValue }
        var title: String {
            switch self {
            case .console: return "Console colours"
            case .accent: return "Accent"
            case .off: return "Off"
            case .custom: return "Custom"
            }
        }
    }

    enum RingMode: String, CaseIterable, Identifiable {
        case rainbow, accent, custom
        var id: String { rawValue }
        var title: String {
            switch self {
            case .rainbow: return "Rainbow"
            case .accent: return "Accent"
            case .custom: return "Custom"
            }
        }
    }

    /// What is behind the pages (Settings, LOOK, Background).
    enum BackgroundStyle: String, CaseIterable, Identifiable {
        /// Colours that swirl and mix, like Google's Gemini app. The default.
        case swirl
        /// One soft glow from the top, as in the mockup.
        case glow
        /// The background colour alone.
        case plain
        var id: String { rawValue }
        var title: String {
            switch self {
            case .swirl: return "Swirl"
            case .glow: return "Glow"
            case .plain: return "Plain"
            }
        }
    }

    /// Where the swirl's colours come from.
    enum SwirlSource: String, CaseIterable, Identifiable {
        /// The accent's colours and their neighbours (a gold accent swirls golds and ambers).
        case accent
        /// Every colour in turn, like Google's Gemini.
        case rainbow
        /// The player's own two or three (`swirlHex`).
        case own
        var id: String { rawValue }
        var title: String {
            switch self {
            case .accent: return "Accent"
            case .rainbow: return "Rainbow"
            case .own: return "My own"
            }
        }
    }

    /// The default accent: gold, as a soft gradient from light to deep gold.
    static let defaultAccent = "FFD66B-F2B233"
    static let defaultBackground = "131316"
    /// The custom glow and ring start from gold.
    static let defaultSingle = "FAC442"

    /// The accent swatches. Gold first, the default. The rest are gradients of two colours.
    static let accentPresets = [
        "FFD66B-F2B233",
        "5AA9FF-2F5BFF",
        "FF5E9C-FF9F43",
        "A66BFF-FF5FB0",
        "6B6B73-D9D9DE",
        "3EE6C1-1E8BE0",
        "FF6A3D-D7263D",
        "9BE15D-00A86B",
    ]
    /// Dark backgrounds only, so white text always reads.
    static let backgroundPresets = ["131316", "000000", "0E1424", "1A1022", "0F1A14", "1C1C1E"]
    /// At most this many saved colours.
    static let savedLimit = 12

    private static let keyPrefix = "continuum.ui.newLook."

    @Published private(set) var version = 0

    @Published var accentHex: String? { didSet { save("accent", accentHex) } }
    @Published var savedSwatches: [String] {
        didSet { save("savedSwatches", savedSwatches.isEmpty ? nil : savedSwatches.joined(separator: ",")) }
    }
    @Published var glowMode: GlowMode { didSet { save("glowMode", glowMode.rawValue) } }
    @Published var glowHex: String? { didSet { save("glow", glowHex) } }
    @Published var ringMode: RingMode { didSet { save("ringMode", ringMode.rawValue) } }
    @Published var ringHex: String? { didSet { save("ring", ringHex) } }
    @Published var ringSpins: Bool { didSet { save("ringSpins", ringSpins ? "on" : "off") } }
    @Published var backgroundHex: String? { didSet { save("background", backgroundHex) } }
    /// "Moving": the swirl (or the glow) behind the pages moves. On by default; off holds the
    /// same picture still.
    @Published var backgroundMoves: Bool { didSet { save("backgroundMoves", backgroundMoves ? nil : "off") } }
    @Published var backgroundStyle: BackgroundStyle {
        didSet { save("backgroundStyle", backgroundStyle == .swirl ? nil : backgroundStyle.rawValue) }
    }
    /// How fast the swirl turns, 0.25 to 3 times the usual.
    @Published var swirlSpeed: Double {
        didSet { save("swirlSpeed", swirlSpeed == 1 ? nil : String(swirlSpeed)) }
    }
    /// How strong the swirl's colours are, 0.2 (faint) to 1 (full).
    @Published var swirlStrength: Double {
        didSet { save("swirlStrength", swirlStrength == Self.defaultSwirlStrength ? nil : String(swirlStrength)) }
    }
    /// The swirl's own colours, two or three joined by dashes (used when the source is My own).
    @Published var swirlHex: String? { didSet { save("swirl", swirlHex) } }
    @Published var swirlSource: SwirlSource {
        didSet { save("swirlSource", swirlSource == .accent ? nil : swirlSource.rawValue) }
    }

    /// The light running round the main buttons, the picked chips and the picked console.
    @Published var buttonLight: Bool { didSet { save("buttonLight", buttonLight ? nil : "off") } }
    /// How fast that light runs round, 0.3 to 3 times the usual.
    @Published var lightSpeed: Double {
        didSet { save("lightSpeed", lightSpeed == 1 ? nil : String(lightSpeed)) }
    }
    /// The glow standing behind a hero's box art (Home, the Game Hub, the pause menu).
    @Published var heroGlow: Bool { didSet { save("heroGlow", heroGlow ? nil : "off") } }
    /// The hero's box art floating gently up and down.
    @Published var boxFloats: Bool { didSet { save("boxFloats", boxFloats ? nil : "off") } }

    static let defaultSwirlStrength = 0.7

    // The New look's cover cards.
    /// Corner rounding, 0 square to 1 very round.
    @Published var coverCorner: Double {
        didSet { save("coverCorner", coverCorner == Self.defaultCorner ? nil : String(coverCorner)) }
    }
    @Published var forceSquare: Bool { didSet { save("forceSquare", forceSquare ? "on" : nil) } }
    @Published var hideTitles: Bool { didSet { save("hideTitles", hideTitles ? "on" : nil) } }

    /// About 12 points on a Medium card, the mockup's corners.
    static let defaultCorner = 0.48

    private init() {
        let d = UserDefaults.standard
        let p = Self.keyPrefix
        accentHex = d.string(forKey: p + "accent").flatMap(Self.validated)
        savedSwatches = (d.string(forKey: p + "savedSwatches") ?? "")
            .split(separator: ",").map(String.init).compactMap(Self.validated)
        glowMode = d.string(forKey: p + "glowMode").flatMap(GlowMode.init(rawValue:)) ?? .console
        glowHex = d.string(forKey: p + "glow").flatMap(Self.validated)
        ringMode = d.string(forKey: p + "ringMode").flatMap(RingMode.init(rawValue:)) ?? .rainbow
        ringHex = d.string(forKey: p + "ring").flatMap(Self.validated)
        ringSpins = d.string(forKey: p + "ringSpins") != "off"
        backgroundHex = d.string(forKey: p + "background").flatMap(Self.validated)
        backgroundMoves = d.string(forKey: p + "backgroundMoves") != "off"
        backgroundStyle = d.string(forKey: p + "backgroundStyle").flatMap(BackgroundStyle.init(rawValue:)) ?? .swirl
        swirlSpeed = d.string(forKey: p + "swirlSpeed").flatMap { Double($0) }
            .map { min(max($0, 0.25), 3) } ?? 1
        swirlStrength = d.string(forKey: p + "swirlStrength").flatMap { Double($0) }
            .map { min(max($0, 0.2), 1) } ?? Self.defaultSwirlStrength
        swirlHex = d.string(forKey: p + "swirl").flatMap { Self.validated($0, maxParts: 3) }
        // Build 206 kept "My own" as just having swirl colours; read that the same way.
        swirlSource = d.string(forKey: p + "swirlSource").flatMap(SwirlSource.init(rawValue:))
            ?? (d.string(forKey: p + "swirl") != nil ? .own : .accent)
        buttonLight = d.string(forKey: p + "buttonLight") != "off"
        lightSpeed = d.string(forKey: p + "lightSpeed").flatMap { Double($0) }
            .map { min(max($0, 0.3), 3) } ?? 1
        heroGlow = d.string(forKey: p + "heroGlow") != "off"
        boxFloats = d.string(forKey: p + "boxFloats") != "off"
        coverCorner = d.string(forKey: p + "coverCorner").flatMap { Double($0) }
            .map { min(max($0, 0), 1) } ?? Self.defaultCorner
        forceSquare = d.string(forKey: p + "forceSquare") == "on"
        hideTitles = d.string(forKey: p + "hideTitles") == "on"
    }

    /// Writes waiting to be saved, newest value per key. Apple's colour picker reports every
    /// step of a drag, so a pick is saved, and `version` (which rebuilds the library pages) moves,
    /// once the finger has rested for a quarter of a second rather than dozens of times a second.
    /// The colour itself still changes on screen at once: the properties above publish at once.
    private var pending: [String: String?] = [:]
    private var flushWork: DispatchWorkItem?
    private var flushesOnLeave: NSObjectProtocol?

    private func save(_ name: String, _ value: String?) {
        pending[Self.keyPrefix + name] = .some(value)
        flushWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.flushPending() }
        flushWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: work)
        if flushesOnLeave == nil {
            // Leaving the app inside that quarter second still keeps the pick.
            flushesOnLeave = NotificationCenter.default.addObserver(
                forName: UIApplication.willResignActiveNotification, object: nil, queue: .main
            ) { [weak self] _ in self?.flushPending() }
        }
    }

    private func flushPending() {
        flushWork?.cancel()
        flushWork = nil
        guard !pending.isEmpty else { return }
        let defaults = UserDefaults.standard
        for (key, value) in pending {
            if let value {
                defaults.set(value, forKey: key)
            } else {
                defaults.removeObject(forKey: key)
            }
        }
        pending.removeAll()
        version &+= 1
    }

    /// Back to gold on charcoal, console glows, the spinning rainbow ring, the moving swirl, the
    /// light round buttons, the glowing floating box, the first box art covers. Saved colours are kept: they are the player's own, deleted one by one.
    func resetToDefaults() {
        accentHex = nil
        glowMode = .console
        glowHex = nil
        ringMode = .rainbow
        ringHex = nil
        ringSpins = true
        backgroundHex = nil
        backgroundMoves = true
        backgroundStyle = .swirl
        swirlSpeed = 1
        swirlStrength = Self.defaultSwirlStrength
        swirlHex = nil
        swirlSource = .accent
        buttonLight = true
        lightSpeed = 1
        heroGlow = true
        boxFloats = true
        coverCorner = Self.defaultCorner
        forceSquare = false
        hideTitles = false
    }

    var isDefault: Bool {
        accentHex == nil && glowMode == .console && glowHex == nil && ringMode == .rainbow
            && ringHex == nil && ringSpins && backgroundHex == nil && backgroundMoves
            && backgroundStyle == .swirl && swirlSpeed == 1 && swirlStrength == Self.defaultSwirlStrength
            && swirlHex == nil && swirlSource == .accent && buttonLight && lightSpeed == 1 && heroGlow && boxFloats
            && coverCorner == Self.defaultCorner && !forceSquare && !hideTitles
    }

    /// Adds a colour to the saved row (newest first), once.
    func saveSwatch(_ value: String) {
        guard let value = Self.validated(value) else { return }
        var list = savedSwatches.filter { $0.caseInsensitiveCompare(value) != .orderedSame }
        list.insert(value, at: 0)
        savedSwatches = Array(list.prefix(Self.savedLimit))
    }

    func deleteSwatch(_ value: String) {
        savedSwatches.removeAll { $0.caseInsensitiveCompare(value) == .orderedSame }
    }

    // MARK: The colours themselves

    /// The accent's colours: one, or the two ends of its gradient.
    var accentStops: [Color] { Self.stops(accentHex ?? Self.defaultAccent).map(Self.color(hex:)) }
    /// The accent as one colour (the middle of a gradient): text, toggles, sliders, glows.
    var accent: Color { Self.color(hex: Self.middle(accentHex ?? Self.defaultAccent)) }
    /// The accent as a fill: the gradient, top left to bottom right.
    var accentFill: LinearGradient {
        let stops = accentStops
        return LinearGradient(colors: stops.count == 1 ? [stops[0], stops[0]] : stops,
                              startPoint: .topLeading, endPoint: .bottomTrailing)
    }
    var background: Color { Self.color(hex: backgroundHex ?? Self.defaultBackground) }

    /// Dark or white, whichever reads on the accent: text on a filled button, a picked chip.
    var onAccent: Color { Self.isLight(hex: Self.middle(accentHex ?? Self.defaultAccent)) ? .black : .white }

    /// The colours that swirl and mix behind the pages (NewLookAura, NewLookMotion.swift), red,
    /// green and blue from 0 to 1: the accent's own colours, two neighbours of them a little
    /// round the colour wheel either way, so the swirl is never one flat colour, and a deep one
    /// for depth. A gold accent swirls golds and ambers; a purple and pink blend swirls purples,
    /// pinks and magentas.
    var swirlRGB: [SIMD3<Double>] {
        let hexes: [String]
        // One colour per ribbon (NewLookSwirl), each clearly its own.
        if swirlSource == .rainbow {
            // Four colours round the whole wheel, starting from the accent's own.
            let start = Self.hsb(hex: Self.stops(accentHex ?? Self.defaultAccent)[0]).0
            hexes = (0..<4).map { Self.hex(h: newLookWrap(start + Double($0) / 4), s: 0.72, b: 0.95) }
        } else if swirlSource == .own, let swirlHex {
            // Exactly the player's own two or three.
            hexes = Self.stops(swirlHex)
        } else {
            // The accent's colours, and a third a little further round the wheel the warm way,
            // so there are three ribbons that differ and stay one family (gold: light gold, deep
            // gold, orange; purple and pink: purple, pink, violet blue).
            let stops = Self.stops(accentHex ?? Self.defaultAccent)
            let first = stops[0]
            let second = stops.count > 1 ? stops[1] : Self.shiftedHue(first, by: -0.06)
            let third = Self.shiftedHue(first, by: stops.count > 1 ? -0.08 : -0.12)
            hexes = [first, second, third]
        }
        return hexes.map { hex in
            let (r, g, b) = Self.components(hex: hex)
            return SIMD3<Double>(r, g, b)
        }
    }

    /// The swirl's own colours as Settings shows them (My own), or the accent's to start from.
    var swirlStops: [String] { Self.stops(swirlHex ?? accentHex ?? Self.defaultAccent) }

    /// The background colour, red, green and blue from 0 to 1.
    var backgroundRGB: SIMD3<Double> {
        let (r, g, b) = Self.components(hex: backgroundHex ?? Self.defaultBackground)
        return SIMD3<Double>(r, g, b)
    }

    /// The same colour at `factor` of its brightness.
    static func dimmed(_ value: String, by factor: Double) -> String {
        let (r, g, b) = components(hex: value)
        return hex(r: r * factor, g: g * factor, b: b * factor)
    }

    /// The ring's colours: the rainbow, or the accent or one colour all the way round.
    var ringColors: [Color] {
        switch ringMode {
        case .rainbow: return [.red, .orange, .yellow, .green, .cyan, .blue, .purple, .pink, .red]
        case .accent:
            let s = accentStops
            return s.count == 1 ? [s[0], s[0].opacity(0.55), s[0]] : [s[0], s[1], s[0]]
        case .custom:
            let c = Self.color(hex: Self.middle(ringHex ?? Self.defaultSingle))
            return [c, c.opacity(0.55), c]
        }
    }

    // MARK: Hex

    /// "FAC442" or "FFD66B-F2B233", upper case, or nil if it is not one of those.
    static func validated(_ raw: String) -> String? { validated(raw, maxParts: 2) }

    /// The same, allowing up to `maxParts` colours (the swirl takes three).
    static func validated(_ raw: String, maxParts: Int) -> String? {
        let parts = raw.uppercased().replacingOccurrences(of: "#", with: "")
            .trimmingCharacters(in: .whitespaces).split(separator: "-").map(String.init)
        guard (1...maxParts).contains(parts.count),
              parts.allSatisfy({ $0.count == 6 && UInt32($0, radix: 16) != nil }) else { return nil }
        return parts.joined(separator: "-")
    }

    static func stops(_ value: String) -> [String] {
        let parts = value.split(separator: "-").map(String.init)
        return parts.isEmpty ? [defaultSingle] : parts
    }

    /// One colour standing for a value: itself, or halfway along its gradient.
    static func middle(_ value: String) -> String {
        let s = stops(value)
        guard s.count > 1 else { return s[0] }
        let (r1, g1, b1) = components(hex: s[0])
        let (r2, g2, b2) = components(hex: s[1])
        return hex(r: (r1 + r2) / 2, g: (g1 + g2) / 2, b: (b1 + b2) / 2)
    }

    static func components(hex: String) -> (Double, Double, Double) {
        let clean = hex.trimmingCharacters(in: CharacterSet(charactersIn: "# "))
        let value = UInt32(clean.prefix(6), radix: 16) ?? 0
        return (Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255,
                Double(value & 0xFF) / 255)
    }

    static func hex(r: Double, g: Double, b: Double) -> String {
        func byte(_ v: Double) -> Int { Int((min(max(v, 0), 1) * 255).rounded()) }
        return String(format: "%02X%02X%02X", byte(r), byte(g), byte(b))
    }

    static func color(hex: String) -> Color {
        let (r, g, b) = components(hex: middle(hex))
        return Color(red: r, green: g, blue: b)
    }

    static func luminance(hex: String) -> Double {
        let (r, g, b) = components(hex: hex)
        return 0.299 * r + 0.587 * g + 0.114 * b
    }

    static func isLight(hex: String) -> Bool { luminance(hex: hex) > 0.6 }

    /// Hue, saturation, brightness, each 0 to 1.
    /// A colour from the system colour picker as hex, or nil if it has no RGB form.
    static func hex(from color: Color) -> String? {
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        guard UIColor(color).getRed(&r, green: &g, blue: &b, alpha: &a) else { return nil }
        return hex(r: Double(r), g: Double(g), b: Double(b))
    }

    static func hsb(hex: String) -> (Double, Double, Double) {
        let (r, g, b) = components(hex: hex)
        var h: CGFloat = 0, s: CGFloat = 0, v: CGFloat = 0, a: CGFloat = 0
        UIColor(red: r, green: g, blue: b, alpha: 1).getHue(&h, saturation: &s, brightness: &v, alpha: &a)
        return (Double(h), Double(s), Double(v))
    }

    static func hex(h: Double, s: Double, b: Double) -> String {
        var r: CGFloat = 0, g: CGFloat = 0, bl: CGFloat = 0, a: CGFloat = 0
        UIColor(hue: h, saturation: s, brightness: b, alpha: 1).getRed(&r, green: &g, blue: &bl, alpha: &a)
        return hex(r: Double(r), g: Double(g), b: Double(bl))
    }

    static func shiftedHue(_ value: String, by amount: Double) -> String {
        let (h, s, b) = hsb(hex: value)
        var nh = (h + amount).truncatingRemainder(dividingBy: 1)
        if nh < 0 { nh += 1 }
        return hex(h: nh, s: max(s, 0.35), b: b)
    }

    /// The same colour, dimmed so its brightness is at most `limit`: a custom background stays
    /// dark, so the white text on it always reads.
    static func darkened(hex: String, to limit: Double = 0.18) -> String {
        let light = luminance(hex: hex)
        guard light > limit else { return String(hex.prefix(6)) }
        let k = limit / light
        let (r, g, b) = components(hex: hex)
        return self.hex(r: r * k, g: g * k, b: b * k)
    }
}

/// A hue (or any fraction) wrapped into 0 up to 1.
func newLookWrap(_ value: Double) -> Double {
    let r = value.truncatingRemainder(dividingBy: 1)
    return r < 0 ? r + 1 : r
}
