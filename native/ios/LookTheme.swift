// Continuum - the second look, and what both looks share.
//
// Brett asked for a second UI the user can pick, with the current one kept as it is. The setting
// is called "Look" and the two choices are "Current" and "New" (his words: no branded names). The
// current look is untouched apart from reading its colours through `ShellPalette`, which now asks
// which look is on. The new look is the dark charcoal and gold one from his only mockup
// (Home, described in .kiro/notes/docs/NEW_LOOK.md); the other screens were designed to match it.
//
// THE CHOICE IS ONE USERDEFAULTS KEY, read once at launch and written on every change. A missing
// key means Current, so an existing install looks exactly as it did. `RootView` observes the
// store and swaps the library shell when it changes, so the switch applies at once, without a
// relaunch. Nothing in the engine, the library, saves or settings depends on which look is on.
//
// PLAY HISTORY lives here too. The new Home shows "Last played" and "Jump Back In", and the app
// did not record when a game was last played or for how long. `PlayHistory` records exactly
// that and nothing more: the moment a game opens, and the time it stays open in the foreground.
// There is no in-game progress anywhere in the app, so the new look shows no progress bar rather
// than an invented percentage.

import SwiftUI
import UIKit

// MARK: - Which look

enum ContinuumLook: String, CaseIterable, Identifiable {
    /// The look the app has always had.
    case current
    /// The dark charcoal and gold look from the mockup.
    case newLook = "new"

    var id: String { rawValue }

    /// The words on the Settings switch. Plain on purpose: Brett rejected branded names.
    var title: String {
        switch self {
        case .current: return "Current"
        case .newLook: return "New"
        }
    }
}

/// The remembered choice. Deliberately NOT main-actor isolated: `ShellPalette` reads it from
/// static colour properties that are used everywhere, including outside view bodies.
final class LookStore: ObservableObject {
    static let shared = LookStore()

    /// A plain `continuum.` key, so it travels with the rest of the settings in a backup.
    static let key = "continuum.ui.look.v1"

    @Published var look: ContinuumLook {
        didSet {
            guard oldValue != look else { return }
            UserDefaults.standard.set(look.rawValue, forKey: Self.key)
            FeedbackCenter.step("look: \(oldValue.rawValue) -> \(look.rawValue)")
        }
    }

    /// Which system the new look's Library is filtered to (a `GameSystem` raw value), or nil for
    /// every game. Held here rather than as view state so it survives a game being played: the
    /// library shell is taken out of the view tree while a game runs.
    @Published var librarySystem: String?

    /// The New look's Library sort (the sort button by its title). Remembered.
    @Published var librarySort: LibrarySort {
        didSet { UserDefaults.standard.set(librarySort.rawValue, forKey: Self.sortKey) }
    }
    static let sortKey = "continuum.ui.newLook.librarySort.v1"

    private init() {
        let stored = UserDefaults.standard.string(forKey: Self.key)
        look = stored.flatMap(ContinuumLook.init(rawValue:)) ?? .current
        librarySort = UserDefaults.standard.string(forKey: Self.sortKey)
            .flatMap(LibrarySort.init(rawValue:)) ?? .name
    }

    static var isNewLook: Bool { shared.look == .newLook }
}

/// How the New look's Library is sorted. Real data only: the name, when a game arrived, and
/// when it was last played (`PlayHistory`).
enum LibrarySort: String, CaseIterable, Identifiable {
    case name
    case added
    case played

    var id: String { rawValue }

    var title: String {
        switch self {
        case .name: return "Name"
        case .added: return "Recently added"
        case .played: return "Last played"
        }
    }

    @MainActor
    func sorted(_ entries: [LibraryEntry], arrivals: [String: Date]) -> [LibraryEntry] {
        func byName(_ a: LibraryEntry, _ b: LibraryEntry) -> Bool {
            GameMetadata.displayTitle(for: a)
                .localizedStandardCompare(GameMetadata.displayTitle(for: b)) == .orderedAscending
        }
        switch self {
        case .name:
            return entries.sorted(by: byName)
        case .added:
            return entries.sorted { a, b in
                let left = arrivals[a.name] ?? a.addedAt
                let right = arrivals[b.name] ?? b.addedAt
                switch (left, right) {
                case let (l?, r?): return l == r ? byName(a, b) : l > r
                case (_?, nil): return true
                case (nil, _?): return false
                case (nil, nil): return byName(a, b)
                }
            }
        case .played:
            let history = PlayHistory.shared
            return entries.sorted { a, b in
                let left = history.record(for: a.name)?.lastPlayed
                let right = history.record(for: b.name)?.lastPlayed
                switch (left, right) {
                case let (l?, r?): return l == r ? byName(a, b) : l > r
                case (_?, nil): return true
                case (nil, _?): return false
                case (nil, nil): return byName(a, b)
                }
            }
        }
    }
}

// MARK: - Play history

/// When a game was last played, and for how long in total, by FILE NAME.
///
/// Keyed by file name for the reason favourites and arrivals are: a path holds the install's
/// container id, which does not survive a reinstall. Stored as two `[String: Double]` maps, the
/// shape `continuum.library.arrivedAt.v1` already uses, so the settings backup carries them the
/// same way.
///
/// Time counts while the game is open and the app is in the foreground. Pausing inside the game
/// still counts, as it would on a console left on; going to the home screen does not.
@MainActor
final class PlayHistory: ObservableObject {
    static let shared = PlayHistory()

    private static let lastPlayedKey = "continuum.library.lastPlayed.v1"
    private static let secondsKey = "continuum.library.playSeconds.v1"

    struct Record: Equatable {
        var lastPlayed: Date
        var seconds: Double
    }

    @Published private(set) var records: [String: Record] = [:]

    /// The game open right now, and when the current foreground stretch of it started.
    private var current: String?
    private var segmentStart: Date?
    private var observers: [NSObjectProtocol] = []

    /// One stretch longer than this is not counted. A phone left in a game overnight with the
    /// screen on is not twelve hours of play anyone wants to see.
    private static let longestStretch: Double = 8 * 3600

    private init() {
        let defaults = UserDefaults.standard
        let dates = defaults.dictionary(forKey: Self.lastPlayedKey) as? [String: Double] ?? [:]
        let seconds = defaults.dictionary(forKey: Self.secondsKey) as? [String: Double] ?? [:]
        var out: [String: Record] = [:]
        for (name, at) in dates {
            out[name] = Record(lastPlayed: Date(timeIntervalSince1970: at),
                               seconds: max(0, seconds[name] ?? 0))
        }
        records = out

        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: UIApplication.didEnterBackgroundNotification,
                                            object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in self?.closeStretch() }
        })
        observers.append(center.addObserver(forName: UIApplication.willEnterForegroundNotification,
                                            object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.current != nil, self.segmentStart == nil else { return }
                self.segmentStart = Date()
            }
        })
    }

    /// A game opened. Called from `EngineHost.activeEntry`'s observer.
    func began(_ name: String) {
        if current != nil { ended() }
        current = name
        let now = Date()
        segmentStart = now
        var record = records[name] ?? Record(lastPlayed: now, seconds: 0)
        record.lastPlayed = now
        records[name] = record
        save()
    }

    /// The game closed.
    func ended() {
        closeStretch()
        current = nil
    }

    /// Adds the stretch since `segmentStart` to the open game's total.
    private func closeStretch() {
        guard let name = current, let start = segmentStart else { return }
        segmentStart = nil
        let now = Date()
        let stretch = now.timeIntervalSince(start)
        var record = records[name] ?? Record(lastPlayed: now, seconds: 0)
        if stretch > 0, stretch < Self.longestStretch {
            record.seconds += stretch
        }
        record.lastPlayed = now
        records[name] = record
        save()
    }

    private func save() {
        let defaults = UserDefaults.standard
        defaults.set(records.mapValues { $0.lastPlayed.timeIntervalSince1970 },
                     forKey: Self.lastPlayedKey)
        defaults.set(records.mapValues { $0.seconds }, forKey: Self.secondsKey)
    }

    func record(for name: String) -> Record? { records[name] }

    /// The games in `library` that have been played, most recent first.
    func recentlyPlayed(in library: [LibraryEntry]) -> [LibraryEntry] {
        library
            .compactMap { entry -> (LibraryEntry, Date)? in
                guard let at = records[entry.name]?.lastPlayed else { return nil }
                return (entry, at)
            }
            .sorted { $0.1 > $1.1 }
            .map { $0.0 }
    }

    /// "1h 45m played", "12m played", or nil under a minute (nothing worth showing yet).
    nonisolated static func playedText(_ record: Record?) -> String? {
        guard let record, record.seconds >= 60 else { return nil }
        let minutes = Int(record.seconds / 60)
        let hours = minutes / 60
        if hours > 0 { return "\(hours)h \(minutes % 60)m played" }
        return "\(minutes)m played"
    }

    /// "Last played: 2d ago". Worked out here rather than with a formatter so the wording is
    /// short and the same on every phone.
    nonisolated static func lastPlayedText(_ record: Record?, now: Date = Date()) -> String? {
        guard let record else { return nil }
        let seconds = max(0, now.timeIntervalSince(record.lastPlayed))
        let ago: String
        if seconds < 60 {
            ago = "just now"
        } else if seconds < 3600 {
            ago = "\(Int(seconds / 60))m ago"
        } else if seconds < 86_400 {
            ago = "\(Int(seconds / 3600))h ago"
        } else {
            ago = "\(Int(seconds / 86_400))d ago"
        }
        return "Last played: \(ago)"
    }
}

// MARK: - The Look switch in Settings

/// Two tappable cards rather than the sliding `SegmentedChoice`, on purpose: switching the look
/// replaces the whole screen, and a selector that changes its value WHILE a finger drags across
/// it would swap the screen out from under that finger, possibly twice.
struct LookPickerCard: View {
    @ObservedObject private var looks = LookStore.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            lookCard
            // The New look's colours, right here under the switch, in Settings' own scroll view
            // (NewLookTheme.swift).
            if looks.look == .newLook {
                NewLookColorPanels()
            }
        }
    }

    private var lookCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            if looks.look == .newLook {
                HStack(spacing: 12) {
                    Image(systemName: SettingsSectionStyle.icon(for: "LOOK"))
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(ShellPalette.gold)
                        .frame(width: 34, height: 34)
                        .background(ShellPalette.gold.opacity(0.16),
                                    in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    Text("Look")
                        .font(.system(size: 17, weight: .bold))
                        .foregroundStyle(.white)
                        .modifier(NewLookSweep())
                }
            } else {
                Text("LOOK")
                    .font(.system(size: 11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(ShellPalette.secondaryText)
            }
            HStack(spacing: 10) {
                ForEach(ContinuumLook.allCases) { option in
                    optionCard(option)
                }
            }
            SettingsNote("Your games and saves stay the same. New is always dark.")

        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(ShellPalette.card,
                    in: RoundedRectangle(cornerRadius: looks.look == .newLook ? 20 : 14,
                                         style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: looks.look == .newLook ? 20 : 14, style: .continuous)
                .strokeBorder(ShellPalette.cardStroke, lineWidth: 0.5)
        )
        .padding(.horizontal, 16)
    }

    private func optionCard(_ option: ContinuumLook) -> some View {
        let selected = looks.look == option
        let tint = option == .newLook ? ShellPalette.gold : ShellPalette.red
        let base = option == .newLook ? ShellPalette.charcoal : Color.black
        return Button {
            guard looks.look != option else { return }
            UISelectionFeedbackGenerator().selectionChanged()
            looks.look = option
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                // A tiny preview of each look: its background with its accent.
                ZStack(alignment: .bottomLeading) {
                    RoundedRectangle(cornerRadius: 8)
                        .fill(base)
                    HStack(spacing: 4) {
                        Capsule().fill(tint).frame(width: 26, height: 8)
                        Capsule().fill(Color.white.opacity(0.25)).frame(width: 18, height: 8)
                    }
                    .padding(8)
                }
                .frame(height: 44)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(Color.white.opacity(0.12), lineWidth: 0.5)
                )
                HStack(spacing: 6) {
                    Text(option.title)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(ShellPalette.text)
                    Spacer(minLength: 0)
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(selected ? ShellPalette.accent : ShellPalette.text.opacity(0.35))
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity)
            .background(ShellPalette.text.opacity(selected ? 0.10 : 0.04),
                        in: RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(selected ? ShellPalette.accent : ShellPalette.text.opacity(0.08),
                                  lineWidth: selected ? 1.5 : 0.5)
            )
            .contentShape(Rectangle())
        }
        .modifier(NewLookButtons(enabled: looks.look == .newLook))
        .accessibilityLabel("\(option.title) look")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

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
    /// "Moving gradient": the background slowly drifts. Off by default.
    @Published var backgroundMoves: Bool { didSet { save("backgroundMoves", backgroundMoves ? "on" : nil) } }

    // The New look's cover cards.
    /// Corner rounding, 0 square to 1 very round.
    @Published var coverCorner: Double {
        didSet { save("coverCorner", coverCorner == Self.defaultCorner ? nil : String(coverCorner)) }
    }
    @Published var forceSquare: Bool { didSet { save("forceSquare", forceSquare ? "on" : nil) } }
    @Published var hideTitles: Bool { didSet { save("hideTitles", hideTitles ? "on" : nil) } }

    static let defaultCorner = 0.35

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
        backgroundMoves = d.string(forKey: p + "backgroundMoves") == "on"
        coverCorner = d.string(forKey: p + "coverCorner").flatMap { Double($0) }
            .map { min(max($0, 0), 1) } ?? Self.defaultCorner
        forceSquare = d.string(forKey: p + "forceSquare") == "on"
        hideTitles = d.string(forKey: p + "hideTitles") == "on"
    }

    private func save(_ name: String, _ value: String?) {
        let key = Self.keyPrefix + name
        if let value {
            UserDefaults.standard.set(value, forKey: key)
        } else {
            UserDefaults.standard.removeObject(forKey: key)
        }
        version &+= 1
    }

    /// Back to gold on charcoal, console glows, the spinning rainbow ring, the first box art
    /// covers. Saved colours are kept: they are the player's own, deleted one by one.
    func resetToDefaults() {
        accentHex = nil
        glowMode = .console
        glowHex = nil
        ringMode = .rainbow
        ringHex = nil
        ringSpins = true
        backgroundHex = nil
        backgroundMoves = false
        coverCorner = Self.defaultCorner
        forceSquare = false
        hideTitles = false
    }

    var isDefault: Bool {
        accentHex == nil && glowMode == .console && glowHex == nil && ringMode == .rainbow
            && ringHex == nil && ringSpins && backgroundHex == nil && !backgroundMoves
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

    /// The colours everything that moves in the New look is drawn from (NewLookMotion.swift):
    /// the accent's colours, two neighbours of them so the masses differ, and a custom ring or
    /// glow colour when one is set. Red, green and blue, 0 to 1.
    var motionRGB: [SIMD3<Double>] {
        let stops = Self.stops(accentHex ?? Self.defaultAccent)
        var hexes = [stops[0], Self.shiftedHue(stops[0], by: 0.11)]
        if stops.count > 1 { hexes.append(stops[1]) }
        hexes.append(Self.shiftedHue(stops[0], by: -0.13))
        if ringMode == .custom, let ringHex { hexes.append(Self.middle(ringHex)) }
        if glowMode == .custom, let glowHex { hexes.append(Self.middle(glowHex)) }
        return hexes.map { hex in
            let (r, g, b) = Self.components(hex: hex)
            return SIMD3<Double>(r, g, b)
        }
    }

    var motionColors: [Color] { motionRGB.map { Color(red: $0.x, green: $0.y, blue: $0.z) } }

    /// The colour of the rising sparks: the cover glow when there is one, else the accent.
    var sparkColor: Color {
        switch glowMode {
        case .off, .console: return accent
        case .accent: return accent
        case .custom: return Self.color(hex: Self.middle(glowHex ?? Self.defaultSingle))
        }
    }

    /// The glow round a cover, or nil for none.
    func glow(for system: GameSystem?) -> Color? {
        switch glowMode {
        case .console: return NewLook.consoleColor(system)
        case .accent: return accent
        case .off: return nil
        case .custom: return Self.color(hex: Self.middle(glowHex ?? Self.defaultSingle))
        }
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
    static func validated(_ raw: String) -> String? {
        let parts = raw.uppercased().replacingOccurrences(of: "#", with: "")
            .trimmingCharacters(in: .whitespaces).split(separator: "-").map(String.init)
        guard (1...2).contains(parts.count),
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
