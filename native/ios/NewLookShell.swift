// Continuum - the pieces of the library in the new look (Settings, LOOK, New).
//
// The design is Brett's one mockup of Home (described in .kiro/notes/docs/NEW_LOOK.md): dark
// charcoal, gold accent, a header with the CONTINUUM wordmark and its ring, a "Last played" hero
// with angled box art, a SYSTEMS carousel with counts, a "Jump Back In" row, and a floating
// translucent tab bar. Library, Favorites and Settings were designed to match it.
//
// NOT A SECOND LIBRARY. `LibraryShell` draws both looks and asks `LookStore` which one at each
// place they differ (the header, Home, the tab bar, the Library's system chips). Everything else,
// select mode, the pop-up line, the layouts and card sizes, the sheets and the drop, is the same
// code in both, so neither look falls behind the other.
//
// Only real data. Play time and "last played" come from `PlayHistory`; the app does not track
// progress, so there are no progress bars.

import SwiftUI
import UIKit

// MARK: - The page behind everything

/// Charcoal with a faint warm glow at the top, as the mockup has behind its hero.
struct NewLookBackdrop: View {
    var body: some View {
        ZStack {
            ShellPalette.charcoal
            RadialGradient(colors: [ShellPalette.gold.opacity(0.10), .clear],
                           center: .topTrailing, startRadius: 10, endRadius: 520)
        }
    }
}

// MARK: - The header's buttons

/// Search, plus and the person button in one glass capsule, as in the mockup.
struct NewLookHeaderTools: View {
    let host: EngineHost
    let status: String
    let cloudConfigured: Bool
    let showsDiagnostics: Bool
    let onSearch: () -> Void

    var body: some View {
        HStack(spacing: 2) {
            icon("magnifyingglass", label: "Search", action: onSearch)
            icon("plus", label: "Import games") {
                host.presentImportPicker()
            }
            .contextMenu {
                Button {
                    host.presentImportPicker()
                } label: {
                    Label("Files", systemImage: "folder")
                }
                Button {
                    host.showImportScreen = true
                } label: {
                    Label("Other sources (Wi-Fi, clipboard, servers)",
                          systemImage: "square.and.arrow.down")
                }
            }
            profileMenu
        }
        .padding(3)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.white.opacity(0.10), lineWidth: 0.5))
    }

    private func icon(_ symbol: String, label: String,
                      action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 38, height: 38)
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }

    /// The person button. There are no accounts in Continuum, so it holds what is about your own
    /// copy of the app: sync, technical details and Settings. The small dot is the Current look's
    /// status dot, so a problem is still visible at a glance.
    private var profileMenu: some View {
        Menu {
            if cloudConfigured {
                Button {
                    host.cloudSync.syncNow(reason: "the menu")
                } label: {
                    Label("Sync now", systemImage: "arrow.triangle.2.circlepath")
                }
            }
            Button {
                host.showDiagnostics.toggle()
            } label: {
                Label(showsDiagnostics ? "Hide technical details" : "Show technical details",
                      systemImage: "info.circle")
            }
            Button {
                host.libraryTab = .settings
            } label: {
                Label("Settings", systemImage: "gearshape")
            }
        } label: {
            Image(systemName: "person.fill")
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 38, height: 38)
                .overlay(alignment: .topTrailing) {
                    Circle()
                        .fill(StatusTint.tint(for: status))
                        .frame(width: 7, height: 7)
                        .padding(7)
                }
                .contentShape(Circle())
        }
        .accessibilityLabel("Sync and technical details")
    }
}

// MARK: - The floating tab bar

struct NewLookTabBar: View {
    let selected: LibraryTab
    let bottomInset: CGFloat
    let onSelect: (LibraryTab) -> Void

    var body: some View {
        HStack(spacing: 4) {
            ForEach(LibraryTab.allCases) { item in
                let isOn = selected == item
                Button {
                    onSelect(item)
                } label: {
                    VStack(spacing: 4) {
                        Image(systemName: NewLook.tabIcon(item))
                            .font(.system(size: 20, weight: .semibold))
                        Text(NewLook.tabTitle(item))
                            .font(.system(size: 11, weight: isOn ? .semibold : .medium))
                    }
                    .foregroundStyle(isOn ? ShellPalette.gold : Color.white.opacity(0.62))
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 9)
                    .background(
                        RoundedRectangle(cornerRadius: 18, style: .continuous)
                            .fill(isOn ? ShellPalette.gold.opacity(0.13) : Color.clear)
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(NewLook.tabTitle(item))
                .accessibilityAddTraits(isOn ? [.isButton, .isSelected] : .isButton)
            }
        }
        .padding(6)
        .background(ShellPalette.charcoal.opacity(0.45),
                    in: RoundedRectangle(cornerRadius: 26, style: .continuous))
        .background(.ultraThinMaterial,
                    in: RoundedRectangle(cornerRadius: 26, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .strokeBorder(Color.white.opacity(0.10), lineWidth: 0.5)
        )
        .shadow(color: .black.opacity(0.45), radius: 16, y: 6)
        .padding(.horizontal, 16)
        .padding(.bottom, max(bottomInset - 10, 10))
    }
}

// MARK: - Home

/// Home in the new look: the hero, SYSTEMS, Jump Back In, Recently Added.
///
/// Its rows are built by `HomeModel.build` only when the library or the play history changes,
/// for the same reason `LibraryShell.rebuildShelves` caches the Current look's shelves.
struct NewLookHome: View {
    @ObservedObject var host: EngineHost
    let artwork: ArtworkStore
    let generation: Int
    @ObservedObject var history: PlayHistory
    @ObservedObject var looks: LookStore
    let topPadding: CGFloat
    let bottomSpace: CGFloat

    @State private var model = HomeModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                if let hero = model.hero {
                    let system = CoreCatalog.system(for: hero)
                    let record = history.record(for: hero.name)
                    NewLookHero(
                        entry: hero,
                        system: system,
                        store: artwork,
                        generation: generation,
                        eyebrow: record == nil ? "RECENTLY ADDED" : "LAST PLAYED",
                        metaLine: NewLook.metaLine(entry: hero, system: system, record: record,
                                                   ps1CoreId: host.ps1CoreChoice.coreId),
                        primaryTitle: host.saveStates.states(for: hero).contains(where: { $0.isAuto })
                            ? "Resume" : "Play",
                        host: host
                    )
                } else {
                    EmptyLibraryNotice(
                        title: "Nothing to play yet",
                        guidance: LibraryShell.emptyGuidance,
                        actionTitle: "Import games"
                    ) {
                        host.presentImportPicker()
                    }
                }

                if !model.systems.isEmpty {
                    systemsBlock
                }

                if !model.jumpBackIn.isEmpty {
                    coverRow(title: "Jump Back In", entries: model.jumpBackIn, playedLines: true)
                }

                if !model.recentlyAdded.isEmpty {
                    coverRow(title: "Recently Added", entries: model.recentlyAdded,
                             playedLines: false)
                }

                Spacer(minLength: bottomSpace)
            }
            .padding(.top, topPadding)
        }
        .ignoresSafeArea(edges: .top)
        .onAppear { rebuild() }
        .onChange(of: host.library) { _ in rebuild() }
        .onChange(of: host.gameArrivals) { _ in rebuild() }
        .onChange(of: history.records) { _ in rebuild() }
    }

    private func rebuild() {
        model = HomeModel.build(library: host.library, arrivals: host.gameArrivals,
                                history: history)
    }

    private var systemsBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text("SYSTEMS")
                    .font(.system(size: 17, weight: .semibold))
                    .tracking(1.2)
                    .foregroundStyle(Color.white.opacity(0.75))
                Spacer(minLength: 8)
                Button("View all") {
                    looks.librarySystem = nil
                    host.libraryTab = .allGames
                }
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Color.white.opacity(0.6))
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 20)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 12) {
                    ForEach(model.systems, id: \.system) { item in
                        NewLookSystemTile(system: item.system, count: item.count,
                                          selected: looks.librarySystem == item.system.rawValue) {
                            looks.librarySystem = item.system.rawValue
                            host.libraryTab = .allGames
                        }
                    }
                }
                .padding(12)
            }
            .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 20))
            .overlay(
                RoundedRectangle(cornerRadius: 20)
                    .strokeBorder(Color.white.opacity(0.07), lineWidth: 0.5)
            )
            .padding(.horizontal, 12)
        }
    }

    private func coverRow(title: String, entries: [LibraryEntry], playedLines: Bool) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 20)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 14) {
                    ForEach(entries) { entry in
                        let system = CoreCatalog.system(for: entry)
                        let record = history.record(for: entry.name)
                        NewLookCoverCard(
                            entry: entry,
                            system: system,
                            store: artwork,
                            generation: generation,
                            isFavourite: host.favourites.contains(entry.name),
                            width: 140,
                            line: playedLines
                                ? (PlayHistory.lastPlayedText(record)
                                    ?? GameMetadata.cardLine(for: entry, system: system))
                                : GameMetadata.cardLine(for: entry, system: system),
                            secondLine: playedLines ? PlayHistory.playedText(record) : nil,
                            host: host
                        )
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 4)
            }
        }
    }
}

// MARK: - The Library's system chips

/// "All" and one chip per system that has games, with counts. Picking one filters the Library
/// (`LookStore.librarySystem`); the SYSTEMS carousel on Home picks the same way.
struct NewLookSystemChips: View {
    let library: [LibraryEntry]
    @ObservedObject var looks: LookStore

    var body: some View {
        let systems = Self.counts(library)
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                chip(title: "All", count: library.count, selected: looks.librarySystem == nil) {
                    looks.librarySystem = nil
                }
                ForEach(systems, id: \.system) { item in
                    chip(title: item.system.displayName, count: item.count,
                         selected: looks.librarySystem == item.system.rawValue) {
                        looks.librarySystem = item.system.rawValue
                    }
                }
            }
            .padding(.horizontal, 16)
        }
    }

    static func counts(_ library: [LibraryEntry]) -> [HomeModel.SystemCount] {
        var counts: [GameSystem: Int] = [:]
        for entry in library {
            if let system = CoreCatalog.system(for: entry) {
                counts[system, default: 0] += 1
            }
        }
        return GameSystem.allCases.compactMap { system -> HomeModel.SystemCount? in
            guard let count = counts[system], count > 0 else { return nil }
            return HomeModel.SystemCount(system: system, count: count)
        }
    }

    /// The library narrowed to the picked system, or all of it when none is picked.
    static func narrowed(_ library: [LibraryEntry], to raw: String?) -> [LibraryEntry] {
        guard let raw, let system = GameSystem(rawValue: raw) else { return library }
        return library.filter { CoreCatalog.system(for: $0) == system }
    }

    private func chip(title: String, count: Int, selected: Bool,
                      action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                Text("\(count)")
                    .font(.system(size: 11, weight: .bold))
                    .opacity(0.7)
            }
            .foregroundStyle(selected ? Color.black : Color.white)
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(selected ? AnyShapeStyle(ShellPalette.gold)
                                 : AnyShapeStyle(Color.white.opacity(0.08)),
                        in: Capsule())
            .overlay(Capsule().strokeBorder(Color.white.opacity(selected ? 0 : 0.08),
                                            lineWidth: 0.5))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

// MARK: - What Home shows

struct HomeModel {
    struct SystemCount {
        let system: GameSystem
        let count: Int
    }

    /// The hero: the last game played, or the newest arrival when nothing has been played yet.
    var hero: LibraryEntry?
    var systems: [SystemCount] = []
    var jumpBackIn: [LibraryEntry] = []
    var recentlyAdded: [LibraryEntry] = []

    @MainActor
    static func build(library: [LibraryEntry], arrivals: [String: Date],
                      history: PlayHistory) -> HomeModel {
        var out = HomeModel()
        guard !library.isEmpty else { return out }

        let played = history.recentlyPlayed(in: library)
        out.jumpBackIn = Array(played.prefix(20))

        out.recentlyAdded = library.sorted { left, right in
            let leftAt = arrivals[left.name] ?? left.addedAt
            let rightAt = arrivals[right.name] ?? right.addedAt
            switch (leftAt, rightAt) {
            case let (leftDate?, rightDate?):
                if leftDate == rightDate {
                    return left.name.localizedStandardCompare(right.name) == .orderedAscending
                }
                return leftDate > rightDate
            case (_?, nil):
                return true
            case (nil, _?):
                return false
            case (nil, nil):
                return left.name.localizedStandardCompare(right.name) == .orderedAscending
            }
        }
        out.hero = played.first ?? out.recentlyAdded.first

        // Counted through the one routing table, in the system's own case order so the carousel
        // does not reshuffle when a game is added.
        var counts: [GameSystem: Int] = [:]
        for entry in library {
            if let system = CoreCatalog.system(for: entry) {
                counts[system, default: 0] += 1
            }
        }
        out.systems = GameSystem.allCases.compactMap { system -> SystemCount? in
            guard let count = counts[system], count > 0 else { return nil }
            return SystemCount(system: system, count: count)
        }
        return out
    }
}

// MARK: - Small shared helpers

enum NewLook {
    static func tabTitle(_ tab: LibraryTab) -> String {
        switch tab {
        case .home: return "Home"
        case .allGames: return "Library"
        case .favorites: return "Favorites"
        case .settings: return "Settings"
        }
    }

    static func tabIcon(_ tab: LibraryTab) -> String {
        switch tab {
        case .home: return "house.fill"
        case .allGames: return "books.vertical.fill"
        case .favorites: return "heart.fill"
        case .settings: return "gearshape.fill"
        }
    }

    /// The core a game will launch on, short ("mGBA", not "mGBA (Game Boy Advance)").
    static func coreName(for entry: LibraryEntry, ps1CoreId: String) -> String? {
        guard let name = CoreCatalog.core(forPath: entry.path, ps1CoreId: ps1CoreId)?.displayName
        else { return nil }
        return name.components(separatedBy: " (").first ?? name
    }

    /// "GAME BOY COLOR · mGBA · 1h 45m played", with only the parts that are known.
    static func metaLine(entry: LibraryEntry, system: GameSystem?, record: PlayHistory.Record?,
                         ps1CoreId: String) -> String {
        var parts: [String] = []
        parts.append(system?.displayName.uppercased() ?? entry.ext.uppercased())
        if let core = coreName(for: entry, ps1CoreId: ps1CoreId) { parts.append(core) }
        if let played = PlayHistory.playedText(record) { parts.append(played) }
        return parts.joined(separator: " \u{2022} ")
    }
}

/// CONTINUUM in light capitals, with the ring from the mockup.
struct NewLookWordmark: View {
    var body: some View {
        HStack(spacing: 8) {
            Text("CONTINUUM")
                .font(.system(size: 21, weight: .light))
                .tracking(2.2)
                .foregroundStyle(.white)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            Circle()
                .strokeBorder(
                    AngularGradient(
                        colors: [.red, .orange, .yellow, .green, .cyan, .blue, .purple, .pink, .red],
                        center: .center
                    ),
                    lineWidth: 4
                )
                .frame(width: 24, height: 24)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Continuum")
        .accessibilityAddTraits(.isHeader)
    }
}

// MARK: - The hero

struct NewLookHero: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let eyebrow: String
    let metaLine: String
    let primaryTitle: String
    let host: EngineHost

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(eyebrow)
                        .font(.system(size: 14, weight: .bold))
                        .tracking(1.2)
                        .foregroundStyle(ShellPalette.gold)
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 30, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(3)
                        .minimumScaleFactor(0.6)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(metaLine)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Color.white.opacity(0.62))
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                NewLookAngledCover(entry: entry, system: system, store: store,
                                   generation: generation, width: 140)
                    .onTapGesture { host.detailEntry = entry }
            }

            HStack(spacing: 12) {
                NewLookPrimaryButton(title: primaryTitle, systemImage: "play.fill") {
                    host.launch(entry: entry)
                }
                NewLookGlassButton(title: "Game Hub", systemImage: "info.circle.fill") {
                    host.detailEntry = entry
                }
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 6)
        .padding(.bottom, 4)
        .background(alignment: .topTrailing) {
            // The cover, blurred, as a warm glow behind the hero.
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: false)
                .frame(width: 260, height: 260)
                .blur(radius: 60)
                .opacity(0.35)
                .allowsHitTesting(false)
        }
        .clipped()
        .contentShape(Rectangle())
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
    }
}

/// Box art turned a little toward the title, with a gold glow, as in the mockup.
struct NewLookAngledCover: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let width: CGFloat

    var body: some View {
        CoverArtView(entry: entry, system: system, store: store,
                     generation: generation, showsCaption: false)
            .frame(width: width, height: (width * 1.15).rounded())
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(Color.white.opacity(0.18), lineWidth: 0.5)
            )
            .shadow(color: ShellPalette.gold.opacity(0.35), radius: 18)
            .shadow(color: .black.opacity(0.6), radius: 10, x: -4, y: 8)
            .rotation3DEffect(.degrees(-16), axis: (x: 0, y: 1, z: 0),
                              anchor: .center, perspective: 0.7)
            .accessibilityHidden(true)
    }
}

// MARK: - Buttons

struct NewLookPrimaryButton: View {
    let title: String
    var systemImage: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 15, weight: .bold))
                }
                Text(title)
                    .font(.system(size: 17, weight: .semibold))
            }
            .foregroundStyle(Color.black)
            .padding(.horizontal, 22)
            .padding(.vertical, 12)
            .frame(minWidth: 128)
            .background(
                LinearGradient(colors: [Color(red: 1.0, green: 0.84, blue: 0.38), ShellPalette.gold],
                               startPoint: .top, endPoint: .bottom),
                in: Capsule()
            )
            .shadow(color: ShellPalette.gold.opacity(0.35), radius: 10, y: 3)
        }
        .buttonStyle(.plain)
    }
}

struct NewLookGlassButton: View {
    let title: String
    var systemImage: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 15, weight: .semibold))
                }
                Text(title)
                    .font(.system(size: 17, weight: .medium))
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .background(.ultraThinMaterial, in: Capsule())
            .overlay(Capsule().strokeBorder(Color.white.opacity(0.14), lineWidth: 0.5))
        }
        .buttonStyle(.plain)
    }
}

/// A round glass button with one glyph.
struct NewLookRoundButton: View {
    let systemImage: String
    var tint: Color = .white
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 46, height: 46)
                .background(.ultraThinMaterial, in: Circle())
                .overlay(Circle().strokeBorder(Color.white.opacity(0.14), lineWidth: 0.5))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

// MARK: - A cover card

struct NewLookCoverCard: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let isFavourite: Bool
    let width: CGFloat
    let line: String?
    let secondLine: String?
    let host: EngineHost

    private var height: CGFloat { (width * 4.0 / 3.0).rounded() }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: width >= 92)
                .frame(width: width, height: height)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .strokeBorder(ShellPalette.gold.opacity(0.28), lineWidth: 1)
                )
                .overlay(alignment: .topLeading) {
                    SystemBadge(text: system?.badge ?? entry.ext.uppercased())
                        .padding(6)
                }
                .overlay(alignment: .topTrailing) {
                    if isFavourite {
                        Image(systemName: "heart.fill")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(ShellPalette.gold)
                            .padding(5)
                            .background(.black.opacity(0.6), in: Circle())
                            .padding(6)
                    }
                }
                .shadow(color: .black.opacity(0.5), radius: 8, y: 4)

            Text(GameMetadata.displayTitle(for: entry))
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(.white)
                .lineLimit(1)
                .padding(.top, 3)
            if let line {
                Text(line)
                    .font(.system(size: 11))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .lineLimit(1)
            }
            if let secondLine {
                Text(secondLine)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(ShellPalette.gold.opacity(0.85))
                    .lineLimit(1)
            }
        }
        .frame(width: width, alignment: .leading)
        .contentShape(Rectangle())
        .onTapGesture {
            host.launch(entry: entry)
        }
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(GameMetadata.displayTitle(for: entry))
        .accessibilityHint("Tap to play, press and hold for the Game Hub")
    }
}

// MARK: - A system tile

/// One console in the SYSTEMS carousel: a drawn console shape (no console photos or logos ship
/// with the app, and those belong to their makers), the name, and how many games it has.
struct NewLookSystemTile: View {
    let system: GameSystem
    let count: Int
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 8) {
                NewLookConsoleGlyph(system: system)
                    .frame(width: 64, height: 58)
                Text(system.displayName.uppercased())
                    .font(.system(size: 10, weight: .heavy))
                    .foregroundStyle(Color.white.opacity(0.9))
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .minimumScaleFactor(0.75)
                    .frame(height: 26)
            }
            .frame(width: 86, height: 112)
            .overlay(alignment: .topTrailing) {
                Text("\(count)")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 6)
                    .frame(minWidth: 22, minHeight: 22)
                    .background(Color(white: 0.28), in: Capsule())
                    .padding(4)
            }
            .background(Color.white.opacity(selected ? 0.08 : 0.0),
                        in: RoundedRectangle(cornerRadius: 14))
            .overlay(
                RoundedRectangle(cornerRadius: 14)
                    .strokeBorder(selected ? ShellPalette.gold : Color.clear, lineWidth: 2)
            )
            .shadow(color: selected ? ShellPalette.gold.opacity(0.55) : .clear, radius: 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(system.displayName), \(count) \(count == 1 ? "game" : "games")")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

/// A simple drawn console: a tall handheld, a wide handheld, a two-screen handheld, a home
/// console, a disc console, a computer, or a phone. Coloured per system.
struct NewLookConsoleGlyph: View {
    let system: GameSystem

    private enum Shape_ { case tall, wide, twoScreen, console, disc, computer, phone }

    private var shape: Shape_ {
        switch system {
        case .gb, .gbc, .pokemini: return .tall
        case .gba, .gg, .lynx, .wswan, .ngp, .psp, .vb: return .wide
        case .ds, .n3ds: return .twoScreen
        case .ps1, .saturn, .segacd, .pcecd, .dreamcast: return .disc
        case .amiga, .c64, .dos, .doom: return .computer
        case .flash, .j2me, .symbian: return .phone
        case .nes, .snes, .sms, .genesis, .fds, .sg1000, .tg16, .atari2600, .n64, .sgx,
             .jaguar, .atari7800, .atari5200, .arcade, .sega32x:
            return .console
        }
    }

    private var body_: Color {
        switch system {
        case .gb: return Color(red: 0.80, green: 0.79, blue: 0.74)
        case .gbc: return Color(red: 0.50, green: 0.36, blue: 0.72)
        case .gba: return Color(red: 0.36, green: 0.34, blue: 0.78)
        case .snes, .nes, .fds: return Color(red: 0.78, green: 0.78, blue: 0.80)
        case .n64: return Color(red: 0.30, green: 0.30, blue: 0.33)
        case .ds, .n3ds: return Color(red: 0.62, green: 0.64, blue: 0.68)
        case .ps1: return Color(red: 0.72, green: 0.72, blue: 0.70)
        case .psp: return Color(red: 0.20, green: 0.20, blue: 0.22)
        case .genesis, .sms, .gg, .segacd, .sega32x, .saturn, .sg1000: return Color(red: 0.22, green: 0.22, blue: 0.25)
        case .dreamcast: return Color(red: 0.88, green: 0.88, blue: 0.86)
        default: return Color(red: 0.45, green: 0.45, blue: 0.48)
        }
    }

    private static let screen = Color(red: 0.55, green: 0.68, blue: 0.45)
    private var screen: Color { Self.screen }

    var body: some View {
        ZStack {
            switch shape {
            case .tall:
                RoundedRectangle(cornerRadius: 6)
                    .fill(body_)
                    .frame(width: 34, height: 52)
                    .overlay(alignment: .top) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(screen)
                            .frame(width: 24, height: 20)
                            .padding(.top, 5)
                    }
                    .overlay(alignment: .bottom) {
                        HStack(spacing: 7) {
                            Image(systemName: "plus").font(.system(size: 8, weight: .black))
                            HStack(spacing: 2) {
                                Circle().frame(width: 5, height: 5)
                                Circle().frame(width: 5, height: 5)
                            }
                        }
                        .foregroundStyle(Color.black.opacity(0.55))
                        .padding(.bottom, 8)
                    }
            case .wide:
                Capsule()
                    .fill(body_)
                    .frame(width: 60, height: 30)
                    .overlay(
                        RoundedRectangle(cornerRadius: 2)
                            .fill(screen.opacity(0.9))
                            .frame(width: 26, height: 18)
                    )
            case .twoScreen:
                VStack(spacing: 2) {
                    RoundedRectangle(cornerRadius: 4).fill(body_).frame(width: 46, height: 26)
                        .overlay(RoundedRectangle(cornerRadius: 1).fill(screen).frame(width: 30, height: 18))
                    RoundedRectangle(cornerRadius: 4).fill(body_).frame(width: 46, height: 26)
                        .overlay(RoundedRectangle(cornerRadius: 1).fill(screen.opacity(0.8)).frame(width: 24, height: 18))
                }
            case .console:
                RoundedRectangle(cornerRadius: 6)
                    .fill(body_)
                    .frame(width: 58, height: 30)
                    .overlay(alignment: .top) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(Color.black.opacity(0.35))
                            .frame(width: 30, height: 6)
                            .padding(.top, 6)
                    }
            case .disc:
                RoundedRectangle(cornerRadius: 6)
                    .fill(body_)
                    .frame(width: 56, height: 34)
                    .overlay(
                        Circle()
                            .strokeBorder(Color.black.opacity(0.3), lineWidth: 2)
                            .frame(width: 22, height: 22)
                    )
            case .computer:
                VStack(spacing: 3) {
                    RoundedRectangle(cornerRadius: 3).fill(body_).frame(width: 44, height: 30)
                        .overlay(RoundedRectangle(cornerRadius: 1).fill(Color.black.opacity(0.6)).frame(width: 36, height: 22))
                    RoundedRectangle(cornerRadius: 2).fill(body_).frame(width: 54, height: 8)
                }
            case .phone:
                RoundedRectangle(cornerRadius: 6)
                    .fill(body_)
                    .frame(width: 28, height: 50)
                    .overlay(alignment: .top) {
                        RoundedRectangle(cornerRadius: 2).fill(screen).frame(width: 20, height: 20).padding(.top, 5)
                    }
            }
        }
        .shadow(color: .black.opacity(0.5), radius: 4, y: 3)
        .accessibilityHidden(true)
    }
}

// MARK: - The Game Hub header (the game card, new look)

/// The top of a game's card in the new look, matching the Home hero: the angled cover over a
/// blurred copy of it, the title, the system, the core it runs on, the real play time, and
/// Play or Resume, the heart and Close. Everything under it (core picker, save states, cheats,
/// manual, achievements, skin, cover art, delete) is the same card the current look has.
struct NewLookHubHeader: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let isFavourite: Bool
    let record: PlayHistory.Record?
    let hasAutoSave: Bool
    let coreName: String?
    let host: EngineHost

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Text("GAME HUB")
                    .font(.system(size: 13, weight: .bold))
                    .tracking(1.4)
                    .foregroundStyle(ShellPalette.gold)
                Spacer(minLength: 0)
                Button {
                    host.detailEntry = nil
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(width: 34, height: 34)
                        .background(.ultraThinMaterial, in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
            }

            HStack(alignment: .center, spacing: 18) {
                NewLookAngledCover(entry: entry, system: system, store: store,
                                   generation: generation, width: 124)
                VStack(alignment: .leading, spacing: 6) {
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 23, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(3)
                        .minimumScaleFactor(0.7)
                        .fixedSize(horizontal: false, vertical: true)
                    Text((system?.displayName ?? entry.ext).uppercased())
                        .font(.system(size: 12, weight: .semibold))
                        .tracking(0.6)
                        .foregroundStyle(Color.white.opacity(0.62))
                    if let coreName {
                        Label("Runs on \(coreName)", systemImage: "cpu")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(Color.white.opacity(0.85))
                    }
                    if let played = PlayHistory.playedText(record) {
                        Label(played, systemImage: "clock")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(ShellPalette.gold)
                    }
                    if let last = PlayHistory.lastPlayedText(record) {
                        Text(last)
                            .font(.system(size: 12))
                            .foregroundStyle(Color.white.opacity(0.55))
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            HStack(spacing: 12) {
                NewLookPrimaryButton(title: hasAutoSave ? "Resume" : "Play",
                                     systemImage: "play.fill") {
                    // The one launch path. Clearing the binding dismisses the sheet first.
                    let target = entry
                    host.detailEntry = nil
                    host.launch(entry: target)
                }
                NewLookRoundButton(systemImage: isFavourite ? "heart.fill" : "heart",
                                   tint: isFavourite ? ShellPalette.gold : .white,
                                   label: isFavourite ? "Remove from favorites"
                                                      : "Add to favorites") {
                    host.toggleFavourite(entry)
                }
                Spacer(minLength: 0)
            }
        }
        .padding(18)
        .background(
            ZStack {
                ShellPalette.charcoal
                CoverArtView(entry: entry, system: system, store: store,
                             generation: generation, showsCaption: false)
                    .blur(radius: 50)
                    .opacity(0.40)
                LinearGradient(colors: [.clear, ShellPalette.charcoal.opacity(0.85)],
                               startPoint: .top, endPoint: .bottom)
            }
            .allowsHitTesting(false)
        )
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .strokeBorder(ShellPalette.gold.opacity(0.25), lineWidth: 1)
        )
    }
}
