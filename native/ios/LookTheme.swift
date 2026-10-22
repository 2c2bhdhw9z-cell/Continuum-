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

// The glow round a game's cover. Here rather than in NewLookColors.swift because it needs the
// app's GameSystem, and NewLookColors.swift is also compiled on its own into the simulator
// preview (tools/newlook-preview).
extension NewLookColors {
    /// The glow round a cover, or nil for none.
    func glow(for system: GameSystem?) -> Color? {
        switch glowMode {
        case .console: return NewLook.consoleColor(system)
        case .accent: return accent
        case .off: return nil
        case .custom: return Self.color(hex: Self.middle(glowHex ?? Self.defaultSingle))
        }
    }
}
