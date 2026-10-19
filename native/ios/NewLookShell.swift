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
/// With Theme, Background, "Moving gradient" on, the slow moving gradient instead
/// (NewLookTheme.swift).
struct NewLookBackdrop: View {
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        if colors.backgroundMoves {
            NewLookMovingBackdrop(colors: colors.movingColors)
        } else {
            ZStack {
                ShellPalette.charcoal
                RadialGradient(colors: [ShellPalette.gold.opacity(0.10), .clear],
                               center: .topTrailing, startRadius: 10, endRadius: 520)
            }
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
                    .foregroundStyle(isOn ? ShellPalette.accentFill
                                          : AnyShapeStyle(Color.white.opacity(0.62)))
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

/// Home in the New look, laid out like the mockup: the LAST PLAYED card, SYSTEMS in one tray,
/// Jump Back In, then Recently added and (Settings, LAYOUT) a shelf per console.
///
/// Its rows are built by `HomeModel.build` only when the library, the play history or a LAYOUT
/// setting changes, for the same reason `LibraryShell.rebuildShelves` caches the Current shelves.
struct NewLookHome: View {
    @ObservedObject var host: EngineHost
    let artwork: ArtworkStore
    let generation: Int
    @ObservedObject var history: PlayHistory
    @ObservedObject var looks: LookStore
    /// Settings, LAYOUT, Console order: the SYSTEMS tray and the per-console shelves follow it.
    @ObservedObject var prefs: LibraryPrefs
    let topPadding: CGFloat
    let bottomSpace: CGFloat

    @State private var model = HomeModel()

    var body: some View {
        // Settings, LAYOUT, "Game card size", read here (the host is observed) and handed to every
        // card, so a new size redraws Home at once.
        let size = host.libraryCardSize
        ScrollView {
            VStack(alignment: .leading, spacing: 30) {
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
                    VStack(alignment: .leading, spacing: 12) {
                        NewLookSectionTitle(title: "SYSTEMS", caps: true, trailing: "View all") {
                            looks.librarySystem = nil
                            host.libraryTab = .allGames
                        }
                        NewLookSystemTray(systems: model.systems,
                                          selected: highlightedSystem) { raw in
                            looks.librarySystem = raw
                            host.libraryTab = .allGames
                        }
                    }
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
}

/// A section's title, as in the mockup: "SYSTEMS" in grey capitals with "View all" on the right,
/// or "Jump Back In" in bold white.
struct NewLookSectionTitle: View {
    let title: String
    var caps = false
    var trailing: String?
    var onTrailing: (() -> Void)?

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            if caps {
                Text(title)
                    .font(.system(size: 18, weight: .semibold))
                    .tracking(1.0)
                    .foregroundStyle(Color.white.opacity(0.72))
            } else {
                Text(title)
                    .font(.system(size: 23, weight: .bold))
                    .foregroundStyle(.white)
            }
            Spacer(minLength: 8)
            if let trailing, let onTrailing {
                Button(trailing, action: onTrailing)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(Color.white.opacity(0.55))
                    .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 20)
    }
}

/// A titled row of glowing cover cards on the New look's Home. The card width follows Settings,
/// LAYOUT, "Game card size". Jump Back In shows "Last played" and the play time under each card:
/// real data only, the app does not track progress.
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
        VStack(alignment: .leading, spacing: 4) {
            NewLookSectionTitle(title: title)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: cardSize.shelfSpacing + 4) {
                    ForEach(entries) { entry in
                        let system = CoreCatalog.system(for: entry)
                        let record = history.record(for: entry.name)
                        NewLookGlowCard(
                            entry: entry,
                            system: system,
                            store: store,
                            generation: generation,
                            isFavourite: favourites.contains(entry.name),
                            width: NewLook.shelfCardWidth(cardSize),
                            cardSize: cardSize,
                            host: host,
                            line: showsPlayLines
                                ? (PlayHistory.lastPlayedText(record)
                                    ?? GameMetadata.cardLine(for: entry, system: system))
                                : GameMetadata.cardLine(for: entry, system: system),
                            secondLine: showsPlayLines ? PlayHistory.playedText(record) : nil,
                            frameAspect: 0.92
                        )
                    }
                }
                // Room for the glow, so the scroll view does not clip it.
                .padding(.horizontal, 20)
                .padding(.vertical, 12)
            }
        }
    }
}

// MARK: - A glowing cover card

/// A cover card in the New look: the WHOLE cover at its own shape (never cropped), with a thin
/// glowing outline in its console's colour (Settings, LOOK, Theme, "Cover glow"), the title and
/// one or two lines under it. A tap plays, a press and hold opens the Game Hub, and in the
/// Library's select mode a tap ticks it, exactly like the Current look's card.
struct NewLookGlowCard: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let isFavourite: Bool
    let width: CGFloat
    let cardSize: LibraryCardSize
    let host: EngineHost
    var line: String?
    var secondLine: String?
    /// Width over height of the cover's space. The cover sits whole inside it, at the bottom.
    var frameAspect: CGFloat = 0.8
    /// Ticked in select mode. Nil when not selecting.
    var selected: Bool?

    /// Theme, Box art, "Square covers": the cover's space is square, and the cover sits whole in
    /// the middle of it.
    private var square: Bool { NewLookColors.shared.forceSquare }
    private var coverHeight: CGFloat { (width / (square ? 1 : frameAspect)).rounded() }

    var body: some View {
        let theme = NewLookColors.shared
        let glow = theme.glow(for: system)
        VStack(alignment: .leading, spacing: cardSize.labelSpacing + 2) {
            styledCover(glow: glow, style: theme.coverStyle)
                .frame(width: width, height: coverHeight)
                .overlay(alignment: .bottomLeading) { selectionMark }

            // Theme, Box art, "Hide game titles".
            if !theme.hideTitles {
                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: cardSize.titleFont + 1, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(1)
            }
            if cardSize.showsCardLine, let line {
                Text(line)
                    .font(.system(size: cardSize.cardLineFont))
                    .foregroundStyle(Color.white.opacity(0.55))
                    .lineLimit(1)
                    .minimumScaleFactor(cardSize.cardLineMinimumScale)
            }
            if cardSize.showsCardLine, let secondLine {
                Text(secondLine)
                    .font(.system(size: cardSize.cardLineFont, weight: .medium))
                    .foregroundStyle(ShellPalette.gold)
                    .lineLimit(1)
            }
        }
        .frame(width: width, alignment: .leading)
        .contentShape(Rectangle())
        .onTapGesture {
            if selected != nil {
                LibrarySelection.shared.toggle(entry)
            } else {
                host.launch(entry: entry)
            }
        }
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(GameMetadata.displayTitle(for: entry)), "
                            + "\(GameMetadata.cardLine(for: entry, system: system))")
        .accessibilityAddTraits(selected == true ? .isSelected : [])
    }

    private func cover(radius: CGFloat, outline: Bool) -> some View {
        CoverArtView(entry: entry, system: system, store: store,
                     generation: generation, showsCaption: width >= 92,
                     captionScale: cardSize.plateCaptionScale,
                     style: .whole(WholeCover(cornerRadius: radius,
                                              alignment: square ? .center : .bottom,
                                              favouriteStar: isFavourite ? cardSize : nil,
                                              outline: outline)))
    }

    /// Theme, Box art, Style: 1 the glow round the cover, 2 the cover on a glass plate edged in
    /// the glow colour, 3 the cover plain with a soft shadow. Corners follow Theme, Box art,
    /// Corners. The cover is always whole.
    @ViewBuilder
    private func styledCover(glow: Color?, style: Int) -> some View {
        let radius = NewLookCoverStyle.radius(width: width)
        switch style {
        case 2:
            let plate = RoundedRectangle(cornerRadius: radius + 4, style: .continuous)
            cover(radius: radius, outline: false)
                .padding(5)
                .background(plate.fill(Color.white.opacity(0.07)))
                .overlay(plate.strokeBorder(glow ?? Color.white.opacity(0.14), lineWidth: 1))
                .shadow(color: (glow ?? .clear).opacity(0.35), radius: 6)
        case 3:
            cover(radius: radius, outline: true)
                .shadow(color: .black.opacity(0.55), radius: 6, y: 4)
        default:
            // Two shadows of the glow colour: a tight one that reads as a thin bright edge, and
            // a soft one that is the glow. They follow the cover's own shape.
            cover(radius: radius, outline: glow == nil)
                .shadow(color: glow ?? .clear, radius: 1.2)
                .shadow(color: (glow ?? .clear).opacity(0.65), radius: 9)
        }
    }

    @ViewBuilder
    private var selectionMark: some View {
        if let selected {
            Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                .font(.system(size: max(14, cardSize.starFont + 8), weight: .semibold))
                .foregroundStyle(selected ? ShellPalette.accent : Color.white)
                .background(Circle().fill(Color.black.opacity(selected ? 0.0 : 0.35)))
                .background(Circle().fill(ShellPalette.onAccent).padding(2).opacity(selected ? 1 : 0))
                .padding(cardSize.badgeInset)
        }
    }
}

// MARK: - The SYSTEMS tray

/// One dark rounded tray holding the console tiles, about five on screen at once, as in the
/// mockup. Home uses it for SYSTEMS; the Library uses it, with an All tile first, to filter.
struct NewLookSystemTray: View {
    let systems: [HomeModel.SystemCount]
    let selected: String?
    /// When set, an All tile comes first, with this many games; picking it passes nil.
    var allCount: Int?
    let onPick: (String?) -> Void

    private var shape: RoundedRectangle { RoundedRectangle(cornerRadius: 22, style: .continuous) }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    if let allCount {
                        NewLookSystemTile(system: nil, count: allCount,
                                          selected: selected == nil) { onPick(nil) }
                            .id("all")
                    }
                    ForEach(systems, id: \.system) { item in
                        NewLookSystemTile(system: item.system, count: item.count,
                                          selected: selected == item.system.rawValue) {
                            onPick(item.system.rawValue)
                        }
                        .id(item.system.rawValue)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 10)
            }
            .onAppear {
                if let selected { proxy.scrollTo(selected, anchor: .center) }
            }
        }
        .background(shape.fill(Color.white.opacity(0.06)))
        .overlay(shape.strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5))
        .clipShape(shape)
        .padding(.horizontal, 12)
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
        out.systems = NewLookSystemTray.counts(library, order: order)
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

    /// A Home shelf card's width at each Settings, LAYOUT, "Game card size". Medium is the
    /// mockup's card.
    static func shelfCardWidth(_ size: LibraryCardSize) -> CGFloat {
        switch size {
        case .extraSmall: return 72
        case .small: return 104
        case .medium: return 136
        case .large: return 176
        }
    }

    /// Each console's own colour, for the glow round its covers (Settings, LOOK, Theme,
    /// "Cover glow", Console colours): the colour its hardware or its boxes are known by.
    static func consoleColor(_ system: GameSystem?) -> Color {
        func c(_ r: Double, _ g: Double, _ b: Double) -> Color { Color(red: r, green: g, blue: b) }
        switch system {
        case .gba: return c(0.36, 0.45, 1.0)
        case .gbc: return c(1.0, 0.70, 0.20)
        case .gb, .pokemini: return c(0.62, 0.80, 0.35)
        case .nes, .fds: return c(0.95, 0.25, 0.25)
        case .snes: return c(0.62, 0.48, 0.95)
        case .n64: return c(0.20, 0.75, 0.40)
        case .ds, .n3ds: return c(0.30, 0.80, 0.95)
        case .vb: return c(0.95, 0.15, 0.20)
        case .ps1, .psp: return c(0.40, 0.55, 0.95)
        case .genesis, .segacd, .sega32x, .sms, .sg1000, .gg: return c(0.20, 0.55, 1.0)
        case .saturn: return c(0.55, 0.60, 0.70)
        case .dreamcast: return c(1.0, 0.50, 0.15)
        case .tg16, .pcecd, .sgx: return c(0.95, 0.55, 0.20)
        case .ngp, .wswan: return c(0.35, 0.65, 1.0)
        case .atari2600, .atari5200, .atari7800, .lynx, .jaguar: return c(0.95, 0.35, 0.25)
        case .arcade: return c(1.0, 0.30, 0.60)
        case .amiga, .c64, .dos, .doom: return c(0.70, 0.70, 0.75)
        case .flash: return c(0.95, 0.30, 0.30)
        case .j2me, .symbian: return c(0.45, 0.85, 0.65)
        case nil: return c(0.98, 0.77, 0.26)
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
    /// Settings, LOOK, Theme: the ring's colour, and whether it spins.
    private var colors: NewLookColors { NewLookColors.shared }
    private var shouldSpin: Bool { colors.ringSpins && !reduceMotion }

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
                    AngularGradient(colors: colors.ringColors, center: .center),
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
        .onAppear { spinning = shouldSpin }
        .onChange(of: reduceMotion) { _ in spinning = shouldSpin }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Continuum")
        .accessibilityAddTraits(.isHeader)
    }
}

// MARK: - The hero

/// The warm glow behind the hero and the Game Hub top, as in the mockup: the accent colour
/// glowing from behind the box art and fading out smoothly. Drawn INSIDE its card and clipped
/// to it, so it never spreads under the header.
struct NewLookGlow: View {
    var body: some View {
        ZStack {
            Color.white.opacity(0.03)
            RadialGradient(colors: [ShellPalette.gold.opacity(0.55), ShellPalette.gold.opacity(0.16),
                                    .clear],
                           center: UnitPoint(x: 0.80, y: 0.38), startRadius: 0, endRadius: 250)
            LinearGradient(colors: [.clear, ShellPalette.charcoal.opacity(0.35)],
                           startPoint: .top, endPoint: .bottom)
        }
    }
}

/// The LAST PLAYED card at the top of the New look's Home: the eyebrow, the title, the console,
/// core and play time on the left, the big upright box on the right (whole, never cropped), and
/// Resume or Play and Game Hub underneath, over the warm glow.
struct NewLookHero: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let eyebrow: String
    let metaLine: String
    let primaryTitle: String
    let host: EngineHost

    private var shape: RoundedRectangle { RoundedRectangle(cornerRadius: 26, style: .continuous) }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(alignment: .center, spacing: 8) {
                VStack(alignment: .leading, spacing: 10) {
                    Text(eyebrow)
                        .font(.system(size: 15, weight: .bold))
                        .tracking(0.8)
                        .foregroundStyle(ShellPalette.gold)
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 28, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(3)
                        .minimumScaleFactor(0.6)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(metaLine)
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Color.white.opacity(0.62))
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                NewLookBoxArt(entry: entry, system: system, store: store,
                              generation: generation, width: 140)
                    .frame(width: 150, height: 176)
                    .contentShape(Rectangle())
                    .onTapGesture { host.detailEntry = entry }
            }

            HStack(spacing: 12) {
                NewLookPrimaryButton(title: primaryTitle, systemImage: "play.fill") {
                    host.launch(entry: entry)
                }
                NewLookGlassButton(title: "Game Hub", systemImage: "info.circle.fill") {
                    host.detailEntry = entry
                }
                Spacer(minLength: 0)
            }
        }
        .padding(20)
        .background(NewLookGlow().clipShape(shape))
        .overlay(shape.strokeBorder(Color.white.opacity(0.07), lineWidth: 0.5))
        .contentShape(shape)
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
        .padding(.horizontal, 12)
    }
}

/// The box art standing up and turned a little toward the title, so it reads as a box, with
/// the accent glowing behind it. The WHOLE cover at its own shape, never cropped.
struct NewLookBoxArt: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let width: CGFloat

    var body: some View {
        CoverArtView(entry: entry, system: system, store: store,
                     generation: generation, showsCaption: true,
                     style: .whole(WholeCover(cornerRadius: 6, alignment: .center)))
            .frame(width: width, height: (width * 1.2).rounded())
            .shadow(color: ShellPalette.gold.opacity(0.45), radius: 22)
            .shadow(color: .black.opacity(0.65), radius: 10, x: -6, y: 10)
            .rotation3DEffect(.degrees(-14), axis: (x: 0, y: 1, z: 0),
                              anchor: .center, perspective: 0.6)
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
            .foregroundStyle(ShellPalette.onAccent)
            .padding(.horizontal, 22)
            .padding(.vertical, 12)
            .frame(minWidth: 128)
            // The accent gradient (Settings, LOOK, Theme, Colours).
            .background(ShellPalette.accentFill, in: Capsule())
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

/// One console in the tray: its photo filling the tile (Wikimedia Commons; every source and
/// licence is in CONSOLE_PHOTOS.md, and no two systems share one), its name in bold italic
/// capitals under it like a console logo, and the count in the top corner. The picked one has a
/// glowing accent border. A nil system is the Library's All tile.
struct NewLookSystemTile: View {
    let system: GameSystem?
    let count: Int
    let selected: Bool
    let action: () -> Void

    private var shape: RoundedRectangle { RoundedRectangle(cornerRadius: 16, style: .continuous) }

    var body: some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Group {
                    if let system {
                        Image("console-\(system.rawValue)")
                            .resizable()
                            .interpolation(.high)
                            .scaledToFit()
                    } else {
                        Image(systemName: "square.grid.2x2.fill")
                            .font(.system(size: 30, weight: .semibold))
                            .foregroundStyle(ShellPalette.gold)
                    }
                }
                .frame(width: 58, height: 58)
                .shadow(color: .black.opacity(0.5), radius: 4, y: 3)

                Text((system.map(NewLook.shortName) ?? "All").uppercased())
                    .font(.system(size: 10, weight: .black).italic())
                    .tracking(-0.2)
                    .foregroundStyle(Color.white.opacity(selected ? 1 : 0.88))
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .minimumScaleFactor(0.7)
                    .frame(height: 26, alignment: .top)
            }
            .padding(.top, 14)
            .padding(.bottom, 6)
            .frame(width: 66)
            .background(shape.fill(Color.white.opacity(selected ? 0.07 : 0.0)))
            .overlay(shape.strokeBorder(selected ? ShellPalette.accentFill : AnyShapeStyle(Color.clear),
                                        lineWidth: 1.5))
            .shadow(color: selected ? ShellPalette.gold.opacity(0.7) : .clear, radius: 8)
            .overlay(alignment: .topTrailing) {
                Text("\(count)")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 5)
                    .frame(minWidth: 20, minHeight: 20)
                    .background(Color(white: 0.30), in: Capsule())
                    .offset(x: -2, y: 2)
            }
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(system.map { "\($0.displayName), \(count) \(count == 1 ? "game" : "games")" }
                            ?? "All games, \(count)")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

// MARK: - The Game Hub header (the game card, new look)

/// The top of a game's card in the new look, matching the Home hero: the standing box over
/// the warm glow, the title, the system, the core it runs on, the real play time, and
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
                NewLookBoxArt(entry: entry, system: system, store: store,
                              generation: generation, width: 116)
                    .frame(width: 126, height: 150)
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
        // The same warm glow as the Home hero, inside the card.
        .background(
            ZStack {
                ShellPalette.charcoal
                NewLookGlow()
            }
            .allowsHitTesting(false)
        )
        .clipShape(RoundedRectangle(cornerRadius: 26, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5)
        )
    }
}
