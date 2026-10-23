// Continuum - the pieces of the library in the new look (Settings, LOOK, New).
//
// THE DESIGN IS THE OWNER'S MOCKUP: .kiro/notes/docs/new-look/mockup-home.jpg (Home). Every other
// page follows its style. How each piece looks is in NewLookDesign.swift, drawn from plain values
// so the simulator preview can draw the same pieces; this file hands them the real games, covers,
// counts and actions.
//
// NOT A SECOND LIBRARY. `LibraryShell` draws both looks and asks `LookStore` which one at each
// place they differ (the header, Home, the tab bar, the Library's system tray). Everything else,
// select mode, the pop-up line, the layouts and card sizes, the sheets and the drop, is the same
// code in both, so neither look falls behind the other.
//
// Only real data. Play time and "last played" come from `PlayHistory`. Progress through a game
// (the mockup's "45% complete") is not shown: the owner does not want it yet (22 October 2026).

import SwiftUI
import UIKit
import GameController

// MARK: - The page behind everything

/// The New look's page: charcoal with the accent's glow from the top (NewLookDesign.swift).
struct NewLookBackdrop: View {
    var body: some View {
        NewLookPage()
    }
}

// MARK: - The header's buttons

/// Search, plus and the person button in one glass capsule, as in the mockup, with the small
/// controller mark under it while a controller is connected.
struct NewLookHeaderTools: View {
    let host: EngineHost
    let status: String
    let cloudConfigured: Bool
    let showsDiagnostics: Bool
    let onSearch: () -> Void
    @ObservedObject private var controllers: PhysicalControllers

    init(host: EngineHost, status: String, cloudConfigured: Bool, showsDiagnostics: Bool,
         onSearch: @escaping () -> Void) {
        self.host = host
        self.status = status
        self.cloudConfigured = cloudConfigured
        self.showsDiagnostics = showsDiagnostics
        self.onSearch = onSearch
        _controllers = ObservedObject(wrappedValue: host.controllers)
    }

    var body: some View {
        NewLookHeaderCapsule {
            Button(action: onSearch) {
                NewLookHeaderBead(systemImage: "magnifyingglass")
            }
            .buttonStyle(NewLookPressStyle(scale: 0.9))
            .accessibilityLabel("Search")

            Button {
                host.presentImportPicker()
            } label: {
                NewLookHeaderBead(systemImage: "plus")
            }
            .buttonStyle(NewLookPressStyle(scale: 0.9))
            .accessibilityLabel("Import games")
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
        .overlay(alignment: .bottomTrailing) {
            if controllers.playablePads > 0 {
                NewLookControllerMark(battery: controllerBattery)
                    .offset(x: -10, y: 20)
                    .allowsHitTesting(false)
            }
        }
    }

    /// The battery of the controller in use, when it reports one.
    private var controllerBattery: Float? {
        guard let battery = (GCController.current ?? GCController.controllers().first)?.battery,
              battery.batteryState != .unknown else { return nil }
        return battery.batteryLevel
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
            NewLookHeaderBead(systemImage: "person.fill")
                .overlay(alignment: .topTrailing) {
                    Circle()
                        .fill(StatusTint.tint(for: status))
                        .frame(width: 7, height: 7)
                        .padding(4)
                }
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
        let tabs = LibraryTab.allCases
        NewLookTabBarFace(
            items: tabs.enumerated().map { index, tab in
                NewLookTabBarFace.Item(id: index, icon: NewLook.tabIcon(tab),
                                       title: NewLook.tabTitle(tab))
            },
            selected: tabs.firstIndex(of: selected) ?? 0,
            bottomPadding: max(bottomInset - 12, 10)
        ) { index in
            if tabs.indices.contains(index) { onSelect(tabs[index]) }
        }
    }
}

// MARK: - Home

/// Home in the New look, laid out like the mockup: the LAST PLAYED hero, SYSTEMS in one tray,
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
            VStack(alignment: .leading, spacing: 0) {
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
                        primaryTitle: NewLook.resumes(hero, host: host) ? "Resume" : "Play",
                        host: host
                    )
                } else {
                    NewLookEmptyHero(
                        title: "Nothing to play yet",
                        line: "Add your games and they'll show up here. Zips work too.",
                        actionTitle: "Import games"
                    ) {
                        host.presentImportPicker()
                    }
                }

                if !model.systems.isEmpty {
                    VStack(alignment: .leading, spacing: 12) {
                        NewLookSectionTitle(title: "Systems", caps: true, trailing: "View all") {
                            looks.librarySystem = nil
                            host.libraryTab = .allGames
                        }
                        // Lit only when a console really is picked (the Library shows the
                        // same one). It used to light the last played game's console too, so
                        // Home showed a console picked while the Library showed All.
                        NewLookSystemTray(systems: model.systems,
                                          selected: looks.librarySystem) { raw in
                            looks.librarySystem = raw
                            host.libraryTab = .allGames
                        }
                    }
                    .padding(.top, NewLookMetrics.sectionGap + 3)
                }

                if !model.jumpBackIn.isEmpty {
                    NewLookShelf(title: "Jump Back In", entries: model.jumpBackIn,
                                 store: artwork, generation: generation,
                                 favourites: host.favourites, host: host, cardSize: size,
                                 history: history, showsPlayLines: true)
                        .padding(.top, NewLookMetrics.sectionGap - 5)
                }

                // Recently added, then (Settings, LAYOUT, "A shelf per system on Home") one shelf
                // per console in the player's console order: the Current look's own shelves,
                // from the same function.
                ForEach(model.shelves) { shelf in
                    NewLookShelf(title: shelf.title, entries: shelf.entries,
                                 store: artwork, generation: generation,
                                 favourites: host.favourites, host: host, cardSize: size,
                                 history: history, showsPlayLines: false)
                        .padding(.top, NewLookMetrics.sectionGap - 10)
                }

                Spacer(minLength: bottomSpace)
            }
            .padding(.top, topPadding + 4)
        }
        .ignoresSafeArea(edges: .top)
        // What scrolls up fades out under the header, so a title never shows through CONTINUUM;
        // nothing is painted behind the header.
        .modifier(NewLookHeaderFade.Mask(enabled: true, headerBottom: topPadding - 6))
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

}

/// A titled row of cover cards on the New look's Home. The card width follows Settings,
/// LAYOUT, "Game card size". Jump Back In shows "Last played: 2d ago" under each card, as in
/// the mockup.
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
        VStack(alignment: .leading, spacing: 2) {
            NewLookSectionTitle(title: title)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: cardSize.shelfSpacing) {
                    ForEach(entries) { entry in
                        let system = CoreCatalog.system(for: entry)
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
                                ? (PlayHistory.lastPlayedText(history.record(for: entry.name))
                                    ?? GameMetadata.cardLine(for: entry, system: system))
                                : GameMetadata.cardLine(for: entry, system: system),
                            frameAspect: NewLookMetrics.cardAspect
                        )
                    }
                }
                // Room for the glow, so the scroll view does not clip it.
                .padding(.horizontal, NewLookMetrics.side - 2)
                .padding(.vertical, 10)
            }
        }
    }
}

// MARK: - A glowing cover card

/// A cover card in the New look (NewLookCardFace): the WHOLE cover in one even frame with a
/// bright rim and soft glow in its console's colour (Settings, LOOK, "Cover glow"), the title and
/// one grey line under it. A tap plays, a press and hold opens the Game Hub, and in the Library's
/// select mode a tap ticks it, exactly like the Current look's card.
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
    /// Width over height of the card's frame, the same for every card in a row. The cover sits
    /// whole inside it, centred.
    var frameAspect: CGFloat = 0.8
    /// Ticked in select mode. Nil when not selecting.
    var selected: Bool?
    @State private var pressed = false

    /// Settings, LOOK, "Square covers": the frame is square, and the cover sits whole in the
    /// middle of it.
    private var square: Bool { NewLookColors.shared.forceSquare }
    private var coverHeight: CGFloat { (width / (square ? 1 : frameAspect)).rounded() }

    var body: some View {
        let theme = NewLookColors.shared
        let radius = NewLookCoverStyle.radius(width: width)
        NewLookCardFace(
            width: width, height: coverHeight, radius: radius,
            glow: theme.glow(for: system),
            // Settings, LOOK, "Hide titles".
            title: theme.hideTitles ? nil : GameMetadata.displayTitle(for: entry),
            line: cardSize.showsCardLine ? line : nil,
            titleFont: cardSize.titleFont + 4,
            lineFont: cardSize.cardLineFont + 3
        ) {
            CoverArtView(entry: entry, system: system, store: store, generation: generation,
                         showsCaption: false, captionScale: cardSize.plateCaptionScale,
                         style: .backdrop)
        } cover: {
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: width >= 92,
                         captionScale: cardSize.plateCaptionScale,
                         style: .whole(WholeCover(cornerRadius: radius,
                                                  alignment: .center,
                                                  favouriteStar: isFavourite ? cardSize : nil,
                                                  outline: false)))
        }
        .overlay(alignment: .topLeading) {
            selectionMark
                .frame(width: width, height: coverHeight, alignment: .bottomLeading)
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if selected != nil {
                LibrarySelection.shared.toggle(entry)
            } else {
                host.launch(entry: entry)
            }
        }
        // Press feedback: scales down a little while the finger is on it, springs back.
        .scaleEffect(pressed ? 0.96 : 1)
        .animation(.spring(response: 0.28, dampingFraction: 0.6), value: pressed)
        .onLongPressGesture(minimumDuration: 0.4, perform: {
            host.detailEntry = entry
        }, onPressingChanged: { pressing in
            pressed = pressing
        })
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(GameMetadata.displayTitle(for: entry)), "
                            + "\(GameMetadata.cardLine(for: entry, system: system))")
        .accessibilityAddTraits(selected == true ? .isSelected : [])
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

/// The mockup's tray of consoles (NewLookTray). Home uses it for SYSTEMS; the Library uses it,
/// with an All tile first, to filter.
struct NewLookSystemTray: View {
    let systems: [HomeModel.SystemCount]
    let selected: String?
    /// When set, an All tile comes first, with this many games; picking it passes nil.
    var allCount: Int?
    let onPick: (String?) -> Void

    var body: some View {
        ScrollViewReader { proxy in
            NewLookTray {
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
            // The picked console in view: when the page opens (a beat later, once the tray has
            // its size, or the scroll does nothing) and whenever another is picked.
            .onAppear {
                guard let selected else { return }
                DispatchQueue.main.async { proxy.scrollTo(selected, anchor: .center) }
            }
            .onChange(of: selected) { picked in
                withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) {
                    proxy.scrollTo(picked ?? "all", anchor: .center)
                }
            }
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
}

/// One console in the tray (NewLookSystemTileFace): its photo (Wikimedia Commons; every source
/// and licence is in CONSOLE_PHOTOS.md, and no two systems share one), its name like a logo, and
/// the count. A nil system is the Library's All tile.
struct NewLookSystemTile: View {
    let system: GameSystem?
    let count: Int
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            NewLookSystemTileFace(image: system.map { Image("console-\($0.rawValue)") },
                                  name: system.map(NewLook.trayName) ?? "ALL",
                                  count: count, selected: selected)
        }
        .buttonStyle(NewLookPressStyle())
        .accessibilityLabel(system.map { "\($0.displayName), \(count) \(count == 1 ? "game" : "games")" }
                            ?? "All games, \(count)")
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
        case .extraSmall: return 76
        case .small: return 108
        case .medium: return NewLookMetrics.cardWidth
        case .large: return 176
        }
    }

    /// The console's name under its picture in the SYSTEMS tray, in capitals like a logo, as in
    /// the mockup ("GAME BOY COLOR", "SUPER NINTENDO", "NINTENDO 64"). It wraps onto two lines.
    static func trayName(_ system: GameSystem) -> String {
        switch system {
        case .nes: return "NES"
        case .fds: return "FAMICOM DISK"
        case .psp: return "PSP"
        case .sg1000: return "SG-1000"
        case .amiga: return "AMIGA"
        case .pokemini: return "POKÉMON MINI"
        case .symbian: return "N-GAGE"
        case .saturn: return "SATURN"
        case .sega32x: return "32X"
        default: return system.displayName.uppercased()
        }
    }

    /// Whether opening this game picks up where it was left (Settings, "Resume where you left
    /// off" on, and an automatic save there), so its button says Resume rather than Play.
    @MainActor
    static func resumes(_ entry: LibraryEntry, host: EngineHost) -> Bool {
        host.saveStates.resumesAutomatically
            && host.saveStates.states(for: entry).contains(where: { $0.isAuto })
    }

    /// Each console's own colour, for the glow round its covers (Settings, LOOK,
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

// MARK: - The hero

/// The LAST PLAYED hero at the top of the New look's Home, as in the mockup
/// (NewLookHeroLayout): the eyebrow, the title, the console, core and play time on the left, the
/// standing box on the right over the warm glow, Resume or Play and Game Hub underneath.
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
        NewLookHeroLayout(
            eyebrow: eyebrow,
            title: GameMetadata.displayTitle(for: entry),
            meta: metaLine,
            primaryTitle: primaryTitle,
            onPrimary: { host.launch(entry: entry) },
            onSecondary: { host.detailEntry = entry }
        ) {
            NewLookBoxArt(entry: entry, system: system, store: store, generation: generation,
                          size: NewLookMetrics.heroArt)
                .modifier(NewLookFloat())
                .contentShape(Rectangle())
                .onTapGesture { host.detailEntry = entry }
        }
        .contentShape(Rectangle())
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
    }
}

/// A game's box standing up (NewLookBoxFace), the WHOLE cover at its own shape, never cropped.
struct NewLookBoxArt: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let size: CGSize

    var body: some View {
        CoverArtView(entry: entry, system: system, store: store,
                     generation: generation, showsCaption: true,
                     style: .box(size))
            .frame(width: size.width, height: size.height)
            .accessibilityHidden(true)
    }
}

// MARK: - The Game Hub header (the game card, new look)

/// The top of a game's card in the new look, in the hero's style: GAME HUB and Close, the title,
/// the console, the core it runs on and the real play time on the left, the standing box on the
/// right over the warm glow, then Play or Resume and the heart. Everything under it is the
/// game card's own blocks, drawn as New-look cards (GameDetailSheet.swift).
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
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("GAME HUB")
                    .font(.system(size: 15, weight: .semibold))
                    .tracking(0.6)
                    .foregroundStyle(colors.accent)
                Spacer(minLength: 0)
                Button {
                    host.detailEntry = nil
                } label: {
                    NewLookHeaderBead(systemImage: "xmark")
                        .background(.ultraThinMaterial, in: Circle())
                }
                .buttonStyle(NewLookPressStyle())
                .accessibilityLabel("Close")
            }

            HStack(alignment: .top, spacing: 6) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 26, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(3)
                        .minimumScaleFactor(0.7)
                        .fixedSize(horizontal: false, vertical: true)
                    Text((system?.displayName ?? entry.ext).uppercased())
                        .font(.system(size: 14))
                        .foregroundStyle(Color.white.opacity(0.55))
                        .padding(.top, 8)
                    VStack(alignment: .leading, spacing: 6) {
                        if let coreName {
                            Label("Runs on \(coreName)", systemImage: "cpu")
                        }
                        if let played = PlayHistory.playedText(record) {
                            Label(played, systemImage: "clock")
                        }
                        if let last = PlayHistory.lastPlayedText(record) {
                            Label(last, systemImage: "calendar")
                        }
                    }
                    .font(.system(size: 14))
                    .foregroundStyle(Color.white.opacity(0.8))
                    .padding(.top, 12)
                }
                .padding(.top, 14)
                .frame(maxWidth: .infinity, alignment: .leading)

                NewLookBoxArt(entry: entry, system: system, store: store, generation: generation,
                              size: CGSize(width: 156, height: 168))
                    .background(NewLookHeroGlow(size: 380))
                    .modifier(NewLookFloat(amplitude: 3))
            }
            .padding(.top, 6)

            HStack(spacing: 10) {
                NewLookPrimaryButton(title: hasAutoSave ? "Resume" : "Play",
                                     systemImage: "play.fill") {
                    // The one launch path. Clearing the binding dismisses the sheet first.
                    let target = entry
                    host.detailEntry = nil
                    host.launch(entry: target)
                }
                NewLookRoundButton(systemImage: isFavourite ? "heart.fill" : "heart",
                                   tint: isFavourite ? colors.accent : .white,
                                   label: isFavourite ? "Remove from favorites"
                                                      : "Add to favorites") {
                    host.toggleFavourite(entry)
                }
                Spacer(minLength: 0)
            }
            .padding(.top, 14)
        }
        .padding(.horizontal, 2)
        // Rises and fades in on a spring as the Game Hub opens (NewLookMotion.swift).
        .modifier(NewLookAppear())
    }
}

// MARK: - Sheets in the New look

/// A sheet's list or form in the New look: the New look's page (held still, so nothing moves
/// behind a sheet over a game) instead of the system's grey lists. The gold tint
/// comes from `RootView`. Nothing at all changes in the Current look.
///
/// Put it on the `List` or `Form` itself, inside its NavigationView / NavigationStack: a
/// navigation container draws its own background over anything put behind it from outside.
struct NewLookListStyle: ViewModifier {
    @ObservedObject private var looks = LookStore.shared

    @ViewBuilder
    func body(content: Content) -> some View {
        if looks.look == .newLook {
            content
                .scrollContentBackground(.hidden)
                .background(NewLookPage(strength: 0.7, allowsMotion: false).ignoresSafeArea())
        } else {
            content
        }
    }
}

/// A sheet's own scrolling page (not a List) in the New look: the same still charcoal and glow.
struct NewLookSheetPage: ViewModifier {
    @ObservedObject private var looks = LookStore.shared

    @ViewBuilder
    func body(content: Content) -> some View {
        if looks.look == .newLook {
            content.background(NewLookPage(strength: 0.7, allowsMotion: false).ignoresSafeArea())
        } else {
            content
        }
    }
}

extension View {
    /// See `NewLookListStyle`.
    func newLookList() -> some View { modifier(NewLookListStyle()) }
    /// See `NewLookSheetPage`.
    func newLookSheetPage() -> some View { modifier(NewLookSheetPage()) }
}

// MARK: - The game card's blocks and other small cards

/// A block's heading: in the Current look the small grey capitals it always had; in the New look
/// the gold icon on its soft gold tile and the title in plain words (NewLookCardHeading).
struct LookBlockHeading: View {
    let title: String

    init(_ title: String) { self.title = title }

    @ViewBuilder
    var body: some View {
        if LookStore.isNewLook {
            NewLookCardHeading(icon: Self.icon(for: title), title: Self.words(title))
        } else {
            Text(title)
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)
        }
    }

    static func icon(for title: String) -> String {
        switch title {
        case "SAVE STATES", "MORE STATES": return "square.stack.3d.down.right.fill"
        case "CHEATS", "THIS GAME'S CHEATS": return "wand.and.stars"
        case "RAM SEARCH": return "magnifyingglass"
        case "COVER ART": return "photo.fill"
        case "ACHIEVEMENTS": return "trophy.fill"
        case "FILES": return "folder.fill"
        case "SKIN": return "paintpalette.fill"
        case "MANUAL": return "book.fill"
        default: return SettingsSectionStyle.icon(for: title)
        }
    }

    /// "SAVE STATES" as "Save states", "THIS GAME'S CHEATS" as "This game's cheats".
    static func words(_ title: String) -> String {
        let lower = title.lowercased()
        return lower.prefix(1).uppercased() + lower.dropFirst()
    }
}

/// A block's card: in the Current look the flat grey card it always had; in the New look the
/// rounded glass card with its hairline (NewLookCardBackground's look).
struct LookBlockCard: ViewModifier {
    var radius: CGFloat = 12

    @ViewBuilder
    func body(content: Content) -> some View {
        if LookStore.isNewLook {
            let shape = RoundedRectangle(cornerRadius: radius + 6, style: .continuous)
            content
                .background(Color.white.opacity(0.055), in: shape)
                .overlay(shape.strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5))
        } else {
            content.background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: radius))
        }
    }
}
