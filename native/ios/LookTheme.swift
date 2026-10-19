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
            if looks.look == .newLook {
                NewLookColorControls()
            }
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
        .buttonStyle(.plain)
        .accessibilityLabel("\(option.title) look")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

// MARK: - The New look's colours (Settings, LOOK, with New on)

/// Every colour of the New look, picked by the player: the accent, the glow round the covers,
/// the ring next to CONTINUUM (and whether it spins), and the page's background tint.
///
/// Stored as hex strings, nil meaning the default, so Reset just forgets them. Deliberately NOT
/// main-actor isolated, for the reason `LookStore` is not: `ShellPalette` reads it from static
/// colour properties. `version` changes with every pick; `RootView` uses it to redraw the
/// library at once (the same way a change of look does).
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

    /// Which colour the editor sheet (`NewLookColorEditor`) is open for, or nil.
    enum Editing: String, Identifiable {
        case accent, glow, ring, background
        var id: String { rawValue }
        var title: String {
            switch self {
            case .accent: return "Accent colour"
            case .glow: return "Cover glow colour"
            case .ring: return "Ring colour"
            case .background: return "Background colour"
            }
        }
    }

    static let defaultAccent = "FAC442"
    static let defaultBackground = "131316"

    /// Swatches offered for the accent. Gold first, the default.
    static let accentPresets = ["FAC442", "FF8A3D", "FF4D6D", "C77DFF", "4D9DFF", "2ED3B7", "7CDB4A", "F2F2F2"]
    /// Dark backgrounds only, so white text always reads.
    static let backgroundPresets = ["131316", "000000", "0E1424", "1A1022", "0F1A14", "1C1C1E"]

    private static let keyPrefix = "continuum.ui.newLook."

    @Published private(set) var version = 0
    @Published var editing: Editing?

    @Published var accentHex: String? { didSet { save("accent", accentHex) } }
    @Published var glowMode: GlowMode { didSet { save("glowMode", glowMode.rawValue) } }
    @Published var glowHex: String? { didSet { save("glow", glowHex) } }
    @Published var ringMode: RingMode { didSet { save("ringMode", ringMode.rawValue) } }
    @Published var ringHex: String? { didSet { save("ring", ringHex) } }
    @Published var ringSpins: Bool { didSet { save("ringSpins", ringSpins ? "on" : "off") } }
    @Published var backgroundHex: String? { didSet { save("background", backgroundHex) } }

    private init() {
        let d = UserDefaults.standard
        let p = Self.keyPrefix
        accentHex = d.string(forKey: p + "accent")
        glowMode = d.string(forKey: p + "glowMode").flatMap(GlowMode.init(rawValue:)) ?? .console
        glowHex = d.string(forKey: p + "glow")
        ringMode = d.string(forKey: p + "ringMode").flatMap(RingMode.init(rawValue:)) ?? .rainbow
        ringHex = d.string(forKey: p + "ring")
        ringSpins = d.string(forKey: p + "ringSpins") != "off"
        backgroundHex = d.string(forKey: p + "background")
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

    /// Back to gold on charcoal, console glows, the spinning rainbow ring.
    func resetToDefaults() {
        accentHex = nil
        glowMode = .console
        glowHex = nil
        ringMode = .rainbow
        ringHex = nil
        ringSpins = true
        backgroundHex = nil
    }

    var isDefault: Bool {
        accentHex == nil && glowMode == .console && glowHex == nil && ringMode == .rainbow
            && ringHex == nil && ringSpins && backgroundHex == nil
    }

    // MARK: The colours themselves

    var accent: Color { Self.color(hex: accentHex ?? Self.defaultAccent) }
    var background: Color { Self.color(hex: backgroundHex ?? Self.defaultBackground) }

    /// Dark or white, whichever reads on the accent: text on a filled button, a picked chip.
    var onAccent: Color { Self.isLight(hex: accentHex ?? Self.defaultAccent) ? .black : .white }

    /// The glow round a cover, or nil for none.
    func glow(for system: GameSystem?) -> Color? {
        switch glowMode {
        case .console: return NewLook.consoleColor(system)
        case .accent: return accent
        case .off: return nil
        case .custom: return Self.color(hex: glowHex ?? Self.defaultAccent)
        }
    }

    /// The ring's colours: the rainbow, or one colour all the way round.
    var ringColors: [Color] {
        switch ringMode {
        case .rainbow: return [.red, .orange, .yellow, .green, .cyan, .blue, .purple, .pink, .red]
        case .accent: return [accent, accent.opacity(0.55), accent]
        case .custom:
            let c = Self.color(hex: ringHex ?? Self.defaultAccent)
            return [c, c.opacity(0.55), c]
        }
    }

    // MARK: Hex

    static func components(hex: String) -> (Double, Double, Double) {
        let clean = hex.trimmingCharacters(in: CharacterSet(charactersIn: "# "))
        let value = UInt32(clean, radix: 16) ?? 0
        return (Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255,
                Double(value & 0xFF) / 255)
    }

    static func color(hex: String) -> Color {
        let (r, g, b) = components(hex: hex)
        return Color(red: r, green: g, blue: b)
    }

    static func isLight(hex: String) -> Bool {
        let (r, g, b) = components(hex: hex)
        return 0.299 * r + 0.587 * g + 0.114 * b > 0.6
    }

    static func hex(from color: Color) -> String? {
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        guard UIColor(color).getRed(&r, green: &g, blue: &b, alpha: &a) else { return nil }
        func byte(_ v: CGFloat) -> Int { Int((min(max(v, 0), 1) * 255).rounded()) }
        return String(format: "%02X%02X%02X", byte(r), byte(g), byte(b))
    }

    /// A custom background is kept dark, so the white text on it always reads: anything brighter
    /// is dimmed to the same colour at a safe brightness.
    static func darkened(hex: String) -> String {
        let (r, g, b) = components(hex: hex)
        let light = 0.299 * r + 0.587 * g + 0.114 * b
        guard light > 0.18 else { return hex }
        let k = 0.18 / light
        func byte(_ v: Double) -> Int { Int((v * k * 255).rounded()) }
        return String(format: "%02X%02X%02X", byte(r), byte(g), byte(b))
    }
}

/// The colour controls under the Look switch, shown while New is on.
struct NewLookColorControls: View {
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("COLOURS")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)

            row("Accent") {
                swatches(NewLookColors.accentPresets,
                         current: colors.accentHex ?? NewLookColors.defaultAccent) {
                    colors.accentHex = $0 == NewLookColors.defaultAccent ? nil : $0
                }
                customButton(.accent)
            }

            row("Cover glow") {
                modeChips(NewLookColors.GlowMode.allCases, current: colors.glowMode,
                          title: { $0.title }) { mode in
                    if mode == .custom { colors.editing = .glow } else { colors.glowMode = mode }
                }
            }

            row("Ring") {
                modeChips(NewLookColors.RingMode.allCases, current: colors.ringMode,
                          title: { $0.title }) { mode in
                    if mode == .custom { colors.editing = .ring } else { colors.ringMode = mode }
                }
            }
            Toggle(isOn: $colors.ringSpins) {
                Text("Spin the ring")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(ShellPalette.text)
            }
            .tint(ShellPalette.accent)

            row("Background") {
                swatches(NewLookColors.backgroundPresets,
                         current: colors.backgroundHex ?? NewLookColors.defaultBackground) {
                    colors.backgroundHex = $0 == NewLookColors.defaultBackground ? nil : $0
                }
                customButton(.background)
            }

            if !colors.isDefault {
                SettingsButton(title: "Reset colours", role: .normal) {
                    colors.resetToDefaults()
                }
            }
        }
    }

    private func row<Content: View>(_ title: String,
                                    @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(ShellPalette.text)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) { content() }
                    .padding(.vertical, 3)
                    .padding(.horizontal, 2)
            }
        }
    }

    private func swatches(_ presets: [String], current: String,
                          pick: @escaping (String) -> Void) -> some View {
        ForEach(presets, id: \.self) { hex in
            let selected = hex.caseInsensitiveCompare(current) == .orderedSame
            Button {
                pick(hex)
            } label: {
                Circle()
                    .fill(NewLookColors.color(hex: hex))
                    .frame(width: 32, height: 32)
                    .overlay(Circle().strokeBorder(Color.white.opacity(0.25), lineWidth: 1))
                    .overlay(
                        Circle()
                            .strokeBorder(selected ? Color.white : Color.clear, lineWidth: 2.5)
                            .padding(-4)
                    )
                    .padding(4)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Colour \(hex)")
            .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
        }
    }

    private func customButton(_ which: NewLookColors.Editing) -> some View {
        Button {
            colors.editing = which
        } label: {
            Circle()
                .fill(AngularGradient(colors: [.red, .yellow, .green, .cyan, .blue, .purple, .red],
                                      center: .center))
                .frame(width: 32, height: 32)
                .overlay(Image(systemName: "plus").font(.system(size: 13, weight: .bold))
                    .foregroundStyle(.white))
                .padding(4)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Pick any colour")
    }

    private func modeChips<Mode: Identifiable & Equatable>(
        _ modes: [Mode], current: Mode, title: @escaping (Mode) -> String,
        pick: @escaping (Mode) -> Void
    ) -> some View {
        ForEach(modes) { mode in
            let selected = mode == current
            Button {
                pick(mode)
            } label: {
                Text(title(mode))
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(selected ? ShellPalette.onAccent : ShellPalette.text)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(selected ? AnyShapeStyle(ShellPalette.accent)
                                         : AnyShapeStyle(Color.white.opacity(0.08)),
                                in: Capsule())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
        }
    }
}

/// The "pick any colour" sheet. Presented from `RootView`, outside the library, so it stays open
/// while the library redraws in the new colour behind it.
struct NewLookColorEditor: View {
    let which: NewLookColors.Editing
    @ObservedObject private var colors = NewLookColors.shared
    @State private var draft: Color = .white
    let onDone: () -> Void

    var body: some View {
        NavigationView {
            VStack(alignment: .leading, spacing: 20) {
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .fill(draft)
                    .frame(height: 120)
                    .overlay(
                        Text("Aa")
                            .font(.system(size: 34, weight: .bold))
                            .foregroundStyle(which == .background ? Color.white : onDraft)
                    )
                ColorPicker("Colour", selection: $draft, supportsOpacity: false)
                    .font(.system(size: 17, weight: .semibold))
                if which == .background {
                    SettingsNote("A bright colour is dimmed so the words stay readable.")
                }
                Spacer(minLength: 0)
            }
            .padding(20)
            .navigationTitle(which.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { onDone() }
                }
            }
        }
        .preferredColorScheme(.dark)
        .presentationDetents([.medium])
        .onAppear { draft = NewLookColors.color(hex: currentHex) }
        .onChange(of: draft) { color in commit(color) }
    }

    private var onDraft: Color {
        (NewLookColors.hex(from: draft).map(NewLookColors.isLight(hex:)) ?? true) ? .black : .white
    }

    private var currentHex: String {
        switch which {
        case .accent: return colors.accentHex ?? NewLookColors.defaultAccent
        case .glow: return colors.glowHex ?? NewLookColors.defaultAccent
        case .ring: return colors.ringHex ?? NewLookColors.defaultAccent
        case .background: return colors.backgroundHex ?? NewLookColors.defaultBackground
        }
    }

    private func commit(_ color: Color) {
        guard let hex = NewLookColors.hex(from: color),
              hex.caseInsensitiveCompare(currentHex) != .orderedSame else { return }
        switch which {
        case .accent: colors.accentHex = hex
        case .glow:
            colors.glowHex = hex
            colors.glowMode = .custom
        case .ring:
            colors.ringHex = hex
            colors.ringMode = .custom
        case .background: colors.backgroundHex = NewLookColors.darkened(hex: hex)
        }
    }
}
