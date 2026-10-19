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
    /// Settings, LAYOUT, Console order: the SYSTEMS row and the per-system shelves follow it.
    @ObservedObject var prefs: LibraryPrefs
    let topPadding: CGFloat
    let bottomSpace: CGFloat

    @State private var model = HomeModel()

    var body: some View {
        // Settings, LAYOUT, "Game card size", read here (the host is observed) and handed to every
        // card, so a new size redraws Home at once, exactly as it does in the Current look.
        let size = host.libraryCardSize
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
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
                    NewLookShelf(title: "Jump Back In", entries: model.jumpBackIn,
                                 store: artwork, generation: generation,
                                 favourites: host.favourites, host: host, cardSize: size,
                                 history: history, showsPlayLines: true)
                }

                // Recently added, then (Settings, LAYOUT, "A shelf per system on Home") one shelf
                // per console in the player's console order: the Current look's own shelves,
                // from the same function.
                ForEach(model.shelves) { shelf in
                    NewLookShelf(title: shelf.title, entries: shelf.entries,
                                 store: artwork, generation: generation,
                                 favourites: host.favourites, host: host, cardSize: size,
                                 history: history, showsPlayLines: false)
                }

                Spacer(minLength: bottomSpace)
            }
            // A little air between the header and the hero card.
            .padding(.top, topPadding + 8)
        }
        .ignoresSafeArea(edges: .top)
        .onAppear { rebuild() }
        .onChange(of: host.library) { _ in rebuild() }
        .onChange(of: host.gameArrivals) { _ in rebuild() }
        .onChange(of: host.showsSystemShelves) { _ in rebuild() }
        .onChange(of: prefs.consoleOrder) { _ in rebuild() }
        .onChange(of: history.records) { _ in rebuild() }
    }

    private func rebuild() {
        model = HomeModel.build(library: host.library, arrivals: host.gameArrivals,
                                history: history,
                                order: prefs.ordered(GameSystem.allCases),
                                systemShelves: host.showsSystemShelves)
    }

    /// The gold tile: the console picked in the Library, or else the last played game's console.
    private var highlightedSystem: String? {
        looks.librarySystem ?? model.hero.flatMap { CoreCatalog.system(for: $0)?.rawValue }
    }

    private var systemsBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
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
                                          selected: highlightedSystem == item.system.rawValue) {
                            looks.librarySystem = item.system.rawValue
                            host.libraryTab = .allGames
                        }
                    }
                }
                // Room for the gold glow, so the scroll view does not clip it.
                .padding(.horizontal, 20)
                .padding(.vertical, 12)
            }
            // The row fades out at both edges rather than chopping a tile off.
            .mask(
                LinearGradient(stops: [.init(color: .clear, location: 0),
                                       .init(color: .black, location: 0.05),
                                       .init(color: .black, location: 0.95),
                                       .init(color: .clear, location: 1)],
                               startPoint: .leading, endPoint: .trailing)
            )
        }
    }
}

/// A titled row of covers on the New look's Home. The cards are the Current look's `CoverCard`,
/// so they show the WHOLE cover at its own shape and follow Settings, LAYOUT, "Game card size"
/// exactly the same way. Jump Back In adds "Last played" and the play time under each card.
struct NewLookShelf: View {
    let title: String
    let entries: [LibraryEntry]
    let store: ArtworkStore
    let generation: Int
    let favourites: Set<String>
    let host: EngineHost
    let cardSize: LibraryCardSize
    let history: PlayHistory
    let showsPlayLines: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 20)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: cardSize.shelfSpacing) {
                    ForEach(entries) { entry in
                        VStack(alignment: .leading, spacing: 4) {
                            CoverCard(
                                entry: entry,
                                system: CoreCatalog.system(for: entry),
                                store: store,
                                generation: generation,
                                isFavourite: favourites.contains(entry.name),
                                width: cardSize.shelfCardWidth,
                                host: host,
                                cardSize: cardSize
                            )
                            if showsPlayLines && cardSize.showsCardLine {
                                playLines(for: entry)
                            }
                        }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 4)
            }
        }
    }

    @ViewBuilder
    private func playLines(for entry: LibraryEntry) -> some View {
        let record = history.record(for: entry.name)
        if let last = PlayHistory.lastPlayedText(record) {
            Text(last)
                .font(.system(size: cardSize.cardLineFont))
                .foregroundStyle(ShellPalette.secondaryText)
                .lineLimit(1)
                .frame(width: cardSize.shelfCardWidth, alignment: .leading)
        }
        if let played = PlayHistory.playedText(record) {
            Text(played)
                .font(.system(size: cardSize.cardLineFont, weight: .medium))
                .foregroundStyle(ShellPalette.gold.opacity(0.85))
                .lineLimit(1)
                .frame(width: cardSize.shelfCardWidth, alignment: .leading)
        }
    }
}

// MARK: - The Library's system chips

/// "All" and one chip per system that has games, with counts. Picking one filters the Library
/// (`LookStore.librarySystem`); the SYSTEMS carousel on Home picks the same way.
struct NewLookSystemChips: View {
    let library: [LibraryEntry]
    @ObservedObject var looks: LookStore
    /// Settings, LAYOUT, Console order: the chips follow it.
    @ObservedObject var prefs: LibraryPrefs

    var body: some View {
        let systems = Self.counts(library, order: prefs.ordered(GameSystem.allCases))
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                chip(title: "All", count: library.count, selected: looks.librarySystem == nil) {
                    looks.librarySystem = nil
                }
                ForEach(systems, id: \.system) { item in
                    chip(title: NewLook.shortName(item.system), count: item.count,
                         selected: looks.librarySystem == item.system.rawValue) {
                        looks.librarySystem = item.system.rawValue
                    }
                }
            }
            .padding(.horizontal, 16)
        }
    }

    /// Each console that has games, with its count, in `order` (the player's console order).
    static func counts(_ library: [LibraryEntry], order: [GameSystem]) -> [HomeModel.SystemCount] {
        var counts: [GameSystem: Int] = [:]
        for entry in library {
            if let system = CoreCatalog.system(for: entry) {
                counts[system, default: 0] += 1
            }
        }
        return order.compactMap { system -> HomeModel.SystemCount? in
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
    /// Recently added, then a shelf per console when that setting is on (`LibraryShell.buildShelves`).
    var shelves: [LibraryShell.Shelf] = []

    @MainActor
    static func build(library: [LibraryEntry], arrivals: [String: Date],
                      history: PlayHistory, order: [GameSystem],
                      systemShelves: Bool) -> HomeModel {
        var out = HomeModel()
        guard !library.isEmpty else { return out }

        let played = history.recentlyPlayed(in: library)
        out.jumpBackIn = Array(played.prefix(20))
        out.shelves = LibraryShell.buildShelves(library: library, arrivals: arrivals,
                                                systemShelves: systemShelves, order: order)
        out.hero = played.first ?? out.shelves.first?.entries.first
        out.systems = NewLookSystemChips.counts(library, order: order)
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

    /// The console's short name for a SYSTEMS tile and a Library chip, so nothing is cut off.
    /// The full name is still on the game card and in the Current look.
    static func shortName(_ system: GameSystem) -> String {
        switch system {
        case .nes: return "NES"
        case .snes: return "SNES"
        case .gb: return "Game Boy"
        case .gbc: return "GBC"
        case .gba: return "GBA"
        case .sms: return "Master System"
        case .gg: return "Game Gear"
        case .genesis: return "Mega Drive"
        case .ps1: return "PlayStation"
        case .ds: return "DS"
        case .fds: return "Famicom Disk"
        case .sg1000: return "SG-1000"
        case .tg16: return "TG-16"
        case .atari2600: return "Atari 2600"
        case .n64: return "N64"
        case .n3ds: return "3DS"
        case .psp: return "PSP"
        case .wswan: return "WonderSwan"
        case .ngp: return "Neo Geo Pocket"
        case .pcecd: return "PC Engine CD"
        case .sgx: return "SuperGrafx"
        case .amiga: return "Amiga"
        case .c64: return "C64"
        case .dos: return "DOS"
        case .doom: return "DOOM"
        case .jaguar: return "Jaguar"
        case .lynx: return "Lynx"
        case .atari7800: return "Atari 7800"
        case .atari5200: return "Atari 5200"
        case .arcade: return "Arcade"
        case .pokemini: return "Pokémon mini"
        case .vb: return "Virtual Boy"
        case .saturn: return "Saturn"
        case .segacd: return "Sega CD"
        case .sega32x: return "32X"
        case .dreamcast: return "Dreamcast"
        case .flash: return "Flash"
        case .j2me: return "J2ME"
        case .symbian: return "N-Gage"
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
    /// iOS Settings, Accessibility, Reduce Motion: the ring stays still.
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var spinning = false

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
                // One slow, even turn every 3.5 seconds, for ever. The animation is tied to this
                // one value, so nothing else in the header moves with it.
                .rotationEffect(.degrees(spinning ? 360 : 0))
                .animation(spinning ? .linear(duration: 3.5).repeatForever(autoreverses: false)
                                    : .default,
                           value: spinning)
        }
        .onAppear { spinning = !reduceMotion }
        .onChange(of: reduceMotion) { still in spinning = !still }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Continuum")
        .accessibilityAddTraits(.isHeader)
    }
}

// MARK: - The hero

/// The LAST PLAYED card at the top of the New look's Home, as in the mockup: a rounded card
/// inset from the screen's edges, over a soft, darkened blur of the game's own cover, with the
/// title, console, core and play time on the left, the tilted cover (whole, never cropped) on the
/// right with room around it, and Play or Resume and Game Hub underneath.
struct NewLookHero: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let eyebrow: String
    let metaLine: String
    let primaryTitle: String
    let host: EngineHost

    private var shape: RoundedRectangle { RoundedRectangle(cornerRadius: 24, style: .continuous) }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(eyebrow)
                        .font(.system(size: 13, weight: .bold))
                        .tracking(1.2)
                        .foregroundStyle(ShellPalette.gold)
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 26, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(3)
                        .minimumScaleFactor(0.6)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(metaLine)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Color.white.opacity(0.68))
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                // The cover sits well inside the card: a fixed space a little bigger than the
                // cover, so the tilt and the glow never reach the card's edge.
                NewLookAngledCover(entry: entry, system: system, store: store,
                                   generation: generation, width: 112)
                    .frame(width: 128, height: 150)
                    .contentShape(Rectangle())
                    .onTapGesture { host.detailEntry = entry }
            }

            HStack(spacing: 10) {
                NewLookPrimaryButton(title: primaryTitle, systemImage: "play.fill") {
                    host.launch(entry: entry)
                }
                NewLookGlassButton(title: "Game Hub", systemImage: "info.circle.fill") {
                    host.detailEntry = entry
                }
                Spacer(minLength: 0)
            }
        }
        .padding(18)
        .background(
            ZStack {
                Color(white: 0.10)
                // The game's own cover, blurred and darkened, so the card takes its colour.
                CoverArtView(entry: entry, system: system, store: store,
                             generation: generation, showsCaption: false, style: .backdrop)
                    .scaleEffect(1.3)
                    .opacity(0.55)
                LinearGradient(colors: [Color.black.opacity(0.15), Color.black.opacity(0.55)],
                               startPoint: .topTrailing, endPoint: .bottomLeading)
            }
            .allowsHitTesting(false)
            .clipShape(shape)
        )
        .overlay(shape.strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5))
        .shadow(color: .black.opacity(0.45), radius: 18, y: 8)
        .contentShape(shape)
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
        .padding(.horizontal, 16)
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
        // THE WHOLE COVER at its own shape (a wide GBA box, a tall NES box), never cropped,
        // centred in its space. The glow and the shadow follow the cover itself.
        CoverArtView(entry: entry, system: system, store: store,
                     generation: generation, showsCaption: true,
                     style: .whole(WholeCover(cornerRadius: 8, alignment: .center)))
            .frame(width: width, height: (width * 1.15).rounded())
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

// MARK: - A system tile

/// One console in the SYSTEMS row, as in the mockup: a photo of the console (Wikimedia Commons;
/// every source and licence is in CONSOLE_PHOTOS.md, and no two systems share one), its short
/// name, and how many games it has. The picked one (or the last played game's console) glows gold.
struct NewLookSystemTile: View {
    let system: GameSystem
    let count: Int
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                Image("console-\(system.rawValue)")
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                    .frame(width: 104, height: 84)
                    .shadow(color: .black.opacity(0.55), radius: 6, y: 4)
                Text(NewLook.shortName(system))
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(Color.white.opacity(selected ? 1 : 0.85))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
            .padding(.top, 14)
            .padding(.bottom, 10)
            .padding(.horizontal, 8)
            .frame(width: 124, height: 136)
            .background(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(Color.white.opacity(selected ? 0.09 : 0.05))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .strokeBorder(selected ? ShellPalette.gold : Color.white.opacity(0.08),
                                  lineWidth: selected ? 2 : 0.5)
            )
            .overlay(alignment: .topTrailing) {
                Text("\(count)")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(selected ? Color.black : Color.white)
                    .padding(.horizontal, 7)
                    .frame(minWidth: 24, minHeight: 24)
                    .background(selected ? ShellPalette.gold : Color(white: 0.26), in: Capsule())
                    .padding(7)
            }
            .shadow(color: selected ? ShellPalette.gold.opacity(0.6) : .clear, radius: 12)
            .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(system.displayName), \(count) \(count == 1 ? "game" : "games")")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
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
