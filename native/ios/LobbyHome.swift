// Continuum - the other Home.
//
// Settings, LAYOUT, "Home", "Last played". Shelves stays the default, and this look never
// changes it. The owner sent the picture on 19 October 2026: the last game opened, the
// consoles in a row, and the games they came back to.
//
// It does not invent a play time or a percent finished. Those were in the picture and the
// app does not know them. A time on this screen is when the game was last opened.

import SwiftUI

// MARK: - The choice

/// Which Home is on. Shelves is the one the app has always had.
enum HomeLook: String, CaseIterable, Identifiable {
    case shelves
    case lastPlayed

    var id: String { rawValue }

    var title: String {
        switch self {
        case .shelves: return "Shelves"
        case .lastPlayed: return "Last played"
        }
    }
}

/// Gold and cream for this Home only. The rest of the app keeps its own red.
enum LobbyPalette {
    static let gold = Color(red: 0.96, green: 0.75, blue: 0.28)
    static let cream = Color(red: 0.86, green: 0.82, blue: 0.64)
    static let dim = Color.white.opacity(0.62)
}

/// Dark only while Last played is the Home. Shelves keeps whatever Appearance is set to.
struct LobbyDarkScheme: ViewModifier {
    let on: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if on {
            content.preferredColorScheme(.dark)
        } else {
            content
        }
    }
}
struct LobbyMark: View {
    var body: some View {
        Circle()
            .stroke(
                AngularGradient(
                    colors: [.red, .orange, .yellow, .green, .cyan, .blue, .purple, .red],
                    center: .center
                ),
                lineWidth: 2.4
            )
            .frame(width: 22, height: 22)
            .accessibilityHidden(true)
    }
}

// MARK: - The page

struct LobbyHome: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var prefs = LibraryPrefs.shared
    let artwork: ArtworkStore
    let generation: Int
    /// Space the top bar already takes, so the first line starts under it.
    let topClearance: CGFloat
    let bottomClearance: CGFloat

    /// Which console the bottom row is showing. Nil is every game they came back to.
    @State private var systemFilter: GameSystem?

    private static let when: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                if let hero = hero {
                    heroBlock(hero)
                } else {
                    EmptyLibraryNotice(
                        title: "Nothing to play yet",
                        guidance: LibraryShell.emptyGuidance,
                        actionTitle: "Import games"
                    ) {
                        host.presentImportPicker()
                    }
                }

                if !systems.isEmpty {
                    systemsBlock
                }

                if !cameBack.isEmpty {
                    cameBackBlock
                }

                Spacer(minLength: bottomClearance)
            }
            .padding(.top, topClearance + 8)
        }
        .background(glow.allowsHitTesting(false))
    }

    // MARK: Last played

    private func heroBlock(_ entry: LibraryEntry) -> some View {
        let system = CoreCatalog.system(for: entry)
        return HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 8) {
                Text(opened(entry) == nil ? "LAST ADDED" : "LAST PLAYED")
                    .font(.system(size: 12, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(LobbyPalette.gold)

                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: 30, weight: .bold))
                    .foregroundStyle(.white)
                    .lineLimit(3)
                    .minimumScaleFactor(0.7)
                    .fixedSize(horizontal: false, vertical: true)

                Text(subtitle(for: entry, system: system))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(LobbyPalette.dim)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 10) {
                    Button {
                        host.launch(entry: entry)
                    } label: {
                        Label("Resume", systemImage: "play.fill")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(.black)
                            .padding(.horizontal, 18)
                            .padding(.vertical, 11)
                            .background(LobbyPalette.gold, in: Capsule())
                    }
                    .buttonStyle(.plain)

                    Button {
                        host.detailEntry = entry
                    } label: {
                        Label("Game Hub", systemImage: "info.circle")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 11)
                            .background(Color.white.opacity(0.14), in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                .padding(.top, 4)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            CoverArtView(entry: entry, system: system, store: artwork,
                         generation: generation, showsCaption: false,
                         style: .whole(WholeCover(cornerRadius: 8, alignment: .center)))
                .frame(width: 132, height: 176)
                .shadow(color: LobbyPalette.gold.opacity(0.35), radius: 18, y: 6)
        }
        .padding(.horizontal, 18)
    }

    // MARK: Consoles

    private var systemsBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("SYSTEMS")
                    .font(.system(size: 13, weight: .semibold))
                    .tracking(1.4)
                    .foregroundStyle(LobbyPalette.dim)
                Spacer()
                Button("View all") {
                    systemFilter = nil
                    host.libraryTab = .allGames
                }
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(LobbyPalette.dim)
            }
            .padding(.horizontal, 18)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 8) {
                    ForEach(systems, id: \.system) { item in
                        systemChip(item.system, count: item.count)
                    }
                }
                .padding(.horizontal, 18)
            }
        }
    }

    private func systemChip(_ system: GameSystem, count: Int) -> some View {
        let on = systemFilter == system
        return Button {
            systemFilter = on ? nil : system
        } label: {
            VStack(spacing: 8) {
                ZStack(alignment: .topTrailing) {
                    ConsoleGlyph(system: system)
                        .frame(width: 78, height: 78)
                        .padding(8)
                        .background(
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .fill(Color.white.opacity(on ? 0.08 : 0.04))
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .stroke(on ? LobbyPalette.gold : Color.clear, lineWidth: 1.5)
                        )
                        .shadow(color: on ? LobbyPalette.gold.opacity(0.55) : .clear, radius: 10)

                    Text(count > 99 ? "99+" : "\(count)")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 2)
                        .background(Color.black.opacity(0.72), in: Capsule())
                        .overlay(Capsule().stroke(Color.white.opacity(0.2), lineWidth: 0.5))
                        .offset(x: 4, y: -4)
                }

                Text(system.displayName.uppercased())
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(on ? LobbyPalette.gold : .white.opacity(0.82))
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .frame(width: 94)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(system.displayName), \(count) games")
    }

    // MARK: Came back to

    private var cameBackBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(systemFilter?.displayName ?? "Jump Back In")
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 18)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 14) {
                    ForEach(cameBack) { entry in
                        cameBackCard(entry)
                    }
                }
                .padding(.horizontal, 18)
            }
        }
    }

    private func cameBackCard(_ entry: LibraryEntry) -> some View {
        let system = CoreCatalog.system(for: entry)
        return Button {
            host.launch(entry: entry)
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                CoverArtView(entry: entry, system: system, store: artwork,
                             generation: generation, showsCaption: false,
                             style: .whole(WholeCover(cornerRadius: 8, alignment: .bottom)))
                    .frame(width: 132, height: 168)

                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .frame(width: 132, alignment: .leading)

                if let opened = opened(entry) {
                    Text("Last played \(Self.when.localizedString(for: opened, relativeTo: Date()))")
                        .font(.system(size: 12))
                        .foregroundStyle(LobbyPalette.dim)
                        .lineLimit(1)
                        .frame(width: 132, alignment: .leading)
                } else if let system {
                    Text(system.displayName)
                        .font(.system(size: 12))
                        .foregroundStyle(LobbyPalette.dim)
                        .lineLimit(1)
                        .frame(width: 132, alignment: .leading)
                }
            }
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button("Game Hub") { host.detailEntry = entry }
        }
    }

    /// A warm pool of light behind the top of the page, the way the picture has it.
    private var glow: some View {
        RadialGradient(
            colors: [LobbyPalette.gold.opacity(0.28), Color.black.opacity(0)],
            center: UnitPoint(x: 0.72, y: 0.12),
            startRadius: 10,
            endRadius: 340
        )
        .ignoresSafeArea()
    }

    // MARK: What to show

    private var shown: [LibraryEntry] {
        let needle = host.librarySearch.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !needle.isEmpty else { return host.library }
        return host.library.filter { entry in
            entry.name.lowercased().contains(needle)
                || GameMetadata.displayTitle(for: entry).lowercased().contains(needle)
        }
    }

    /// The game at the top: the one opened most recently, or the newest one added.
    private var hero: LibraryEntry? {
        let pool = shown
        let played = pool.compactMap { entry -> (LibraryEntry, Date)? in
            guard let date = host.lastPlayed[entry.name] else { return nil }
            return (entry, date)
        }
        if let latest = played.max(by: { $0.1 < $1.1 })?.0 {
            return latest
        }
        return pool.max { left, right in
            let leftAt = host.gameArrivals[left.name] ?? left.addedAt
            let rightAt = host.gameArrivals[right.name] ?? right.addedAt
            switch (leftAt, rightAt) {
            case let (leftDate?, rightDate?):
                return leftDate == rightDate
                    ? left.name > right.name
                    : leftDate < rightDate
            case (nil, _?):
                return true
            default:
                return false
            }
        }
    }

    private var systems: [(system: GameSystem, count: Int)] {
        var counts: [GameSystem: Int] = [:]
        for entry in shown {
            if let system = CoreCatalog.system(for: entry) {
                counts[system, default: 0] += 1
            }
        }
        return prefs.ordered(GameSystem.allCases).compactMap { system in
            guard let count = counts[system], count > 0 else { return nil }
            return (system, count)
        }
    }

    /// The row under the consoles. The game already at the top is left out.
    private var cameBack: [LibraryEntry] {
        let heroName = hero?.name
        var pool = shown.filter { $0.name != heroName }
        if let systemFilter {
            pool = pool.filter { CoreCatalog.system(for: $0) == systemFilter }
        }
        return pool.sorted { left, right in
            let leftAt = host.lastPlayed[left.name] ?? host.gameArrivals[left.name] ?? left.addedAt ?? .distantPast
            let rightAt = host.lastPlayed[right.name] ?? host.gameArrivals[right.name] ?? right.addedAt ?? .distantPast
            return leftAt > rightAt
        }
    }

    private func opened(_ entry: LibraryEntry) -> Date? {
        host.lastPlayed[entry.name]
    }

    private func subtitle(for entry: LibraryEntry, system: GameSystem?) -> String {
        var parts: [String] = []
        if let system {
            parts.append(system.displayName.uppercased())
        }
        if let name = CoreCatalog.core(forPath: entry.path, ps1CoreId: host.ps1CoreChoice.coreId)?.displayName {
            parts.append(name)
        }
        if let opened = opened(entry) {
            parts.append(Self.when.localizedString(for: opened, relativeTo: Date()))
        }
        return parts.joined(separator: "  ·  ")
    }
}

// MARK: - A console, drawn, not a photo of one

/// A small picture of the console, drawn here. Not a photograph and not a logo.
struct ConsoleGlyph: View {
    let system: GameSystem

    var body: some View {
        switch system {
        case .gb:
            handheld(body: Color(white: 0.78), screen: Color(red: 0.55, green: 0.62, blue: 0.38), wide: false)
        case .gbc:
            handheld(body: Color(red: 0.62, green: 0.40, blue: 0.78), screen: Color(red: 0.45, green: 0.55, blue: 0.28), wide: false)
        case .gba:
            handheld(body: Color(red: 0.42, green: 0.36, blue: 0.72), screen: Color(white: 0.15), wide: true)
        case .gg:
            handheld(body: Color(white: 0.22), screen: Color(red: 0.35, green: 0.55, blue: 0.32), wide: false)
        case .lynx, .ngp, .wswan, .pokemini:
            handheld(body: Color(white: 0.35), screen: Color(white: 0.12), wide: true)
        case .snes:
            deck(body: Color(white: 0.72), accent: Color(red: 0.55, green: 0.25, blue: 0.45))
        case .nes:
            deck(body: Color(white: 0.82), accent: Color(red: 0.7, green: 0.15, blue: 0.15))
        case .genesis, .sms, .sg1000, .sega32x:
            deck(body: Color(white: 0.18), accent: Color(white: 0.45))
        case .n64:
            n64
        case .ds, .n3ds:
            clamshell
        case .psp:
            handheld(body: Color(white: 0.16), screen: Color(white: 0.08), wide: true)
        case .ps1:
            deck(body: Color(white: 0.9), accent: Color(white: 0.35))
        default:
            monogram
        }
    }

    private func handheld(body: Color, screen: Color, wide: Bool) -> some View {
        RoundedRectangle(cornerRadius: wide ? 10 : 8, style: .continuous)
            .fill(body)
            .overlay(
                RoundedRectangle(cornerRadius: 3, style: .continuous)
                    .fill(screen)
                    .padding(.horizontal, wide ? 16 : 14)
                    .padding(.top, wide ? 14 : 12)
                    .padding(.bottom, wide ? 28 : 34)
            )
            .overlay(alignment: .bottom) {
                HStack(spacing: wide ? 18 : 10) {
                    Circle().fill(Color.black.opacity(0.35)).frame(width: 8, height: 8)
                    Circle().fill(Color.black.opacity(0.45)).frame(width: wide ? 10 : 14, height: wide ? 10 : 14)
                }
                .padding(.bottom, 8)
            }
            .padding(6)
    }

    private func deck(body: Color, accent: Color) -> some View {
        RoundedRectangle(cornerRadius: 6, style: .continuous)
            .fill(body)
            .overlay(alignment: .leading) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(accent)
                    .frame(width: 10)
                    .padding(.vertical, 10)
                    .padding(.leading, 8)
            }
            .overlay(alignment: .trailing) {
                Circle()
                    .fill(Color.black.opacity(0.25))
                    .frame(width: 16, height: 16)
                    .padding(.trailing, 10)
            }
            .padding(.horizontal, 4)
            .padding(.vertical, 18)
    }

    private var n64: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(Color(white: 0.55))
                .frame(height: 36)
            Capsule()
                .fill(Color(white: 0.28))
                .frame(width: 28, height: 10)
                .offset(y: -16)
        }
        .padding(10)
    }

    private var clamshell: some View {
        VStack(spacing: 2) {
            RoundedRectangle(cornerRadius: 4, style: .continuous)
                .fill(Color(white: 0.25))
                .overlay(
                    RoundedRectangle(cornerRadius: 2).fill(Color(white: 0.08))
                        .padding(6)
                )
            RoundedRectangle(cornerRadius: 4, style: .continuous)
                .fill(Color(white: 0.55))
        }
        .padding(10)
    }

    private var monogram: some View {
        Text(system.badge)
            .font(.system(size: 13, weight: .heavy))
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.white.opacity(0.12))
            )
            .padding(12)
    }
}
