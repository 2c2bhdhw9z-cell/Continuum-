// Continuum - the other Home.
//
// Settings, LAYOUT, "Home", "Last played". Shelves stays the default, and this look never
// changes it. The owner sent the picture on 19 October 2026: the last game opened, the
// consoles in a row, and the games they came back to.
//
// It does not invent a play time or a percent finished. Those were in the picture and the
// app does not know them. A time on this screen is when the game was last opened.

import SwiftUI
import UIKit

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
                lineWidth: 3.2
            )
            .frame(width: 26, height: 26)
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
        return HStack(alignment: .center, spacing: 8) {
            VStack(alignment: .leading, spacing: 8) {
                Text("LAST PLAYED")
                    .font(.system(size: 12, weight: .heavy))
                    .tracking(1.8)
                    .foregroundStyle(LobbyPalette.gold)

                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: 28, weight: .bold))
                    .foregroundStyle(.white)
                    .lineLimit(3)
                    .minimumScaleFactor(0.65)
                    .fixedSize(horizontal: false, vertical: true)

                Text(subtitle(for: entry, system: system))
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Color.white.opacity(0.55))
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 10) {
                    Button {
                        host.launch(entry: entry)
                    } label: {
                        Label("Resume", systemImage: "play.fill")
                            .font(.system(size: 16, weight: .bold))
                            .foregroundStyle(.black)
                            .padding(.horizontal, 18)
                            .padding(.vertical, 12)
                            .background(LobbyPalette.gold, in: Capsule())
                            .shadow(color: LobbyPalette.gold.opacity(0.45), radius: 10, y: 2)
                    }
                    .buttonStyle(.plain)

                    Button {
                        host.detailEntry = entry
                    } label: {
                        Label("Game Hub", systemImage: "info.circle")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .background(Color.white.opacity(0.16), in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                .padding(.top, 6)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            LobbyBox(entry: entry, system: system, store: artwork, generation: generation)
        }
        .padding(.horizontal, 18)
    }

    // MARK: Consoles

    private var systemsBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("SYSTEMS")
                    .font(.system(size: 13, weight: .semibold))
                    .tracking(1.6)
                    .foregroundStyle(Color.white.opacity(0.45))
                Spacer()
                Button("View all") {
                    systemFilter = nil
                    host.libraryTab = .allGames
                }
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Color.white.opacity(0.45))
            }
            .padding(.horizontal, 20)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 6) {
                    ForEach(systems, id: \.system) { item in
                        systemChip(item.system, count: item.count)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 14)
            }
            .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 22, style: .continuous))
            .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
            .padding(.horizontal, 16)
        }
    }

    private func systemChip(_ system: GameSystem, count: Int) -> some View {
        let on = systemFilter == system
        return Button {
            systemFilter = on ? nil : system
        } label: {
            VStack(spacing: 6) {
                ZStack(alignment: .topTrailing) {
                    LobbyConsolePhoto(system: system)
                        .frame(width: 74, height: 74)
                    Text(count > 99 ? "99+" : "\(count)")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(minWidth: 18, minHeight: 18)
                        .padding(.horizontal, 4)
                        .background(Color(white: 0.28), in: Circle())
                        .offset(x: 6, y: -4)
                }
                lobbyName(system, on: on)
                    .frame(width: 86)
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(on ? LobbyPalette.gold : Color.clear, lineWidth: 1.6)
                    .shadow(color: on ? LobbyPalette.gold.opacity(0.85) : .clear, radius: 8)
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(system.displayName), \(count) games")
    }

    @ViewBuilder
    private func lobbyName(_ system: GameSystem, on: Bool) -> some View {
        let lines = system.lobbyLines
        VStack(spacing: 0) {
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                if system == .gbc && line == "COLOR" {
                    HStack(spacing: 0) {
                        ForEach(Array(line.enumerated()), id: \.offset) { index, character in
                            Text(String(character))
                                .foregroundStyle(Color(hue: Double(index) / Double(line.count), saturation: 0.85, brightness: 1))
                        }
                    }
                    .font(.system(size: 9, weight: .heavy))
                } else {
                    Text(line)
                        .font(.system(size: 9, weight: .heavy))
                        .foregroundStyle(on ? .white : Color.white.opacity(0.82))
                }
            }
        }
        .multilineTextAlignment(.center)
        .lineLimit(2)
    }

    // MARK: Came back to

    private var cameBackBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(systemFilter?.displayName ?? "Jump Back In")
                .font(.system(size: 26, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 20)

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
                             style: .whole(WholeCover(cornerRadius: 6, alignment: .center, outline: true)))
                    .frame(width: 148, height: 148)
                    .shadow(color: .black.opacity(0.45), radius: 8, y: 4)

                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .frame(width: 148, alignment: .leading)

                if let opened = opened(entry) {
                    Text("Last played: \(Self.when.localizedString(for: opened, relativeTo: Date()))")
                        .font(.system(size: 12))
                        .foregroundStyle(Color.white.opacity(0.5))
                        .lineLimit(1)
                        .frame(width: 148, alignment: .leading)
                } else if let system {
                    Text(system.displayName)
                        .font(.system(size: 12))
                        .foregroundStyle(Color.white.opacity(0.5))
                        .lineLimit(1)
                        .frame(width: 148, alignment: .leading)
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


// MARK: - The standing box

/// The cover stood up, the way a box sits on a shelf. The spine is the left edge.
struct LobbyBox: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int

    var body: some View {
        ZStack(alignment: .leading) {
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: false,
                         style: .whole(WholeCover(cornerRadius: 3, alignment: .center, outline: false)))
                .frame(width: 118, height: 156)
                .padding(.leading, 11)
            LinearGradient(
                colors: [Color(white: 0.16), Color(white: 0.42), Color(white: 0.22)],
                startPoint: .leading,
                endPoint: .trailing
            )
            .frame(width: 12, height: 156)
            .overlay(
                Text((system?.badge ?? "").uppercased())
                    .font(.system(size: 7, weight: .heavy))
                    .foregroundStyle(.white.opacity(0.8))
                    .fixedSize()
                    .rotationEffect(.degrees(-90))
            )
        }
        .frame(width: 129, height: 156)
        .clipShape(RoundedRectangle(cornerRadius: 3, style: .continuous))
        .shadow(color: LobbyPalette.gold.opacity(0.45), radius: 18, y: 4)
        .shadow(color: .black.opacity(0.5), radius: 8, y: 6)
    }
}

// MARK: - The machines

extension GameSystem {
    /// A photo in the asset catalog, when this console has one.
    var lobbyAsset: String { "lobby-\(rawValue)" }

    /// Short lines that fit under the photo. The long system name does not.
    var lobbyLines: [String] {
        switch self {
        case .gb: return ["GAME BOY"]
        case .gbc: return ["GAME BOY", "COLOR"]
        case .gba: return ["GAME BOY", "ADVANCE"]
        case .nes: return ["NES"]
        case .snes: return ["SUPER", "NINTENDO"]
        case .n64: return ["NINTENDO", "64"]
        case .ds: return ["NINTENDO", "DS"]
        case .n3ds: return ["NINTENDO", "3DS"]
        case .genesis: return ["MEGA", "DRIVE"]
        case .sms: return ["MASTER", "SYSTEM"]
        case .gg: return ["GAME", "GEAR"]
        case .ps1: return ["PLAYSTATION"]
        case .psp: return ["PSP"]
        case .dreamcast: return ["DREAMCAST"]
        case .saturn: return ["SATURN"]
        case .arcade: return ["ARCADE"]
        default: return [badge]
        }
    }
}

struct LobbyConsolePhoto: View {
    let system: GameSystem

    var body: some View {
        if UIImage(named: system.lobbyAsset) != nil {
            Image(system.lobbyAsset)
                .resizable()
                .scaledToFit()
        } else {
            Text(system.badge)
                .font(.system(size: 13, weight: .heavy))
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
    }
}
