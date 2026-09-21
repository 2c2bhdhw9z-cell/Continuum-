// Continuum - the library shell: the screen the app opens into.
//
// This replaces the monospace debug HUD over a plain List. The structure is the design reference,
// docs/library-mobile.png: a dark full-bleed shell, a top bar with the logo, a search field and an
// import button, a featured hero with real cover art, horizontal shelves of cover cards each badged
// with its system, a thin status strip, and a bottom tab bar whose active tab is red.
//
// THREE THINGS IN HERE ARE LOAD-BEARING AND NOT COSMETIC.
//
//  1. THE SHELL IS OPAQUE AND THE CANVAS IS NEVER UNMOUNTED. `RootView` keeps exactly one
//     MetalCanvasView at the bottom of its stack for the app's lifetime, because `attachMetal`
//     hands the engine that view's CAMetalLayer and the engine then owns the surface. A
//     NavigationStack push that replaced the canvas would tear that layer down and force a second
//     attach on the one path that cannot be tested anywhere but a device. So this shell covers the
//     canvas opaquely and the player screen reveals it, exactly as Stage 1 left it. There is no
//     NavigationStack here at all: the detail view is a sheet, which needs no stack and cannot
//     disturb what is underneath.
//
//  2. THE TAB BAR IS HAND-BUILT RATHER THAN A TabView, and that is a deliberate departure from
//     FEAT-003's wording. The reference tab bar is text only, red when active, on an opaque black
//     bar with a hairline above it. Getting that out of TabView means mutating UITabBarAppearance
//     globally and accepting a translucent bar over a live Metal layer, and the thin status strip
//     would have to be duplicated into all four tabs or overlaid anyway. A selection enum plus an
//     HStack is the same four-destination shell with none of that, and it is what makes the strip
//     and the bar one opaque unit at the bottom.
//
//  3. THE HEAVY VIEWS OBSERVE NOTHING. `EngineHost` publishes telemetry on every display-link
//     frame, so this file observes it once, here, and hands plain values to the cards. See the
//     header of LibraryCards.swift.

import SwiftUI
import UIKit

// MARK: - Which tab

enum LibraryTab: String, CaseIterable, Identifiable {
    case home
    case allGames
    case favorites
    case settings

    var id: String { rawValue }

    /// The tab bar label. American spelling on Favorites because that is what the design reference
    /// shows on the bar.
    var title: String {
        switch self {
        case .home: return "Home"
        case .allGames: return "All Games"
        case .favorites: return "Favorites"
        case .settings: return "Settings"
        }
    }
}

/// How All Games arranges itself. The user asked to be able to change the layout, and this is the
/// part of that which is real today: it needs no engine change, it persists, and it does something
/// visible. The touch-control layout editor is a bigger piece of work and is recorded in FEAT-006.
enum LibraryLayout: String, CaseIterable, Identifiable {
    case grid
    case list

    var id: String { rawValue }

    var title: String {
        switch self {
        case .grid: return "Grid"
        case .list: return "List"
        }
    }
}

// MARK: - The shell

struct LibraryShell: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var artwork: ArtworkStore

    /// The real safe-area insets and window size, measured from UIKit. See `SafeAreaProbe` for why
    /// they are not read from a GeometryProxy.
    @State private var metrics = ShellMetrics()

    /// The empty state, carried over from the debug library verbatim in substance. It names the
    /// accepted extensions from the one routing table, explains selecting a .cue with every .bin
    /// track, and mentions the Files app fallback that UIFileSharingEnabled already provides. On a
    /// sideloaded build there is nothing else to tell the user what to do next.
    static let emptyGuidance =
        "No games yet. Tap the plus button and select your ROM files: "
        + CoreCatalog.extensionList(CoreCatalog.launchableExtensions)
        + ". For a PS1 disc select the .cue together with every .bin track it names (in the "
        + "picker: Select, tap each file, Open). Files you drop into the Continuum folder in "
        + "the Files app show up here too."

    private var topBarHeight: CGFloat { 52 }
    private var bottomBarHeight: CGFloat { 84 }

    /// How tall the featured hero is.
    ///
    /// A fraction of the window rather than a fixed number, clamped at both ends. The design
    /// reference's hero fills most of a large phone, and the same fixed height on a small one would
    /// leave no room for a shelf underneath it, so there would be nothing to suggest the library
    /// scrolls.
    private var heroHeight: CGFloat {
        let available = metrics.size.height
        guard available > 0 else { return 430 }
        return min(430, max(300, available * 0.52))
    }

    var body: some View {
        ZStack(alignment: .top) {
            // Opaque, and that is the point: the canvas is still mounted and ticking underneath,
            // and a library you can see a game through is the debug overlay this screen replaces.
            Color.black
                .ignoresSafeArea()

            content
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)

            VStack(spacing: 0) {
                topBar
                Spacer(minLength: 0)
            }

            VStack(spacing: 0) {
                Spacer(minLength: 0)
                bottomChrome
            }
        }
        .background(SafeAreaProbe { metrics = $0 })
        // A sheet rather than a pushed view, so nothing in the view tree under it is replaced.
        .sheet(item: $host.detailEntry) { entry in
            GameDetailSheet(entry: entry, host: host, artwork: artwork)
        }
    }

    // MARK: Top bar

    private var topBar: some View {
        HStack(spacing: 10) {
            logoTile
            searchField
            importButton
            statusDot
        }
        .padding(.horizontal, 14)
        .frame(height: topBarHeight)
        .padding(.top, max(metrics.insets.top, 8))
        .background(
            // A gradient rather than a solid fill, so the hero art reads as full-bleed behind the
            // bar exactly as it does in the design reference, while the controls stay legible.
            LinearGradient(
                colors: [.black.opacity(0.92), .black.opacity(0.55), .black.opacity(0)],
                startPoint: .top,
                endPoint: .bottom
            )
            .ignoresSafeArea(edges: .top)
        )
    }

    private var logoTile: some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 3)
                .fill(ShellPalette.accent)
                .frame(width: 9, height: 22)
            Text("CONTINUUM")
                .font(.system(size: 14, weight: .heavy))
                .tracking(1.6)
                .foregroundStyle(.white)
        }
        .accessibilityAddTraits(.isHeader)
    }

    private var searchField: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Color.white.opacity(0.55))
            // The placeholder counts what is actually there, from the one library array.
            TextField("Search \(host.library.count) titles...", text: $host.librarySearch)
                .font(.system(size: 14))
                .foregroundStyle(.white)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled(true)
                .submitLabel(.search)
            if !host.librarySearch.isEmpty {
                Button {
                    host.librarySearch = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 13))
                        .foregroundStyle(Color.white.opacity(0.5))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear the search")
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 36)
        .background(ShellPalette.surface, in: Capsule())
        .frame(maxWidth: .infinity)
    }

    /// The import affordance. Wired to the EXISTING `presentImportPicker()` and to nothing else:
    /// that path is device-verified, including its retained delegate and its multi-select of a .cue
    /// with every .bin track, and it must not be reimplemented or wrapped.
    private var importButton: some View {
        Button {
            host.presentImportPicker()
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(ShellPalette.surfaceStrong, in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Import games")
    }

    private var statusDot: some View {
        Button {
            host.showDiagnostics.toggle()
        } label: {
            Circle()
                .fill(StatusTint.tint(for: host.status))
                .frame(width: 10, height: 10)
                .frame(width: 36, height: 36)
                .background(ShellPalette.surface, in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Diagnostics")
    }

    // MARK: The bottom: status strip, then tab bar

    private var bottomChrome: some View {
        VStack(spacing: 0) {
            if host.showDiagnostics {
                // Capped and scrollable so a long error cannot push the tab bar off screen, which
                // would leave the user with no way back to Home.
                ScrollView {
                    DiagnosticsPanel(host: host)
                        .padding(.horizontal, 12)
                        .padding(.bottom, 6)
                }
                .frame(maxHeight: 260)
            }
            statusStrip
            tabBar
        }
        .background(Color.black)
    }

    /// The thin strip: always visible, one tap from the whole diagnostic block.
    ///
    /// On a sideloaded build this line is the only debugger there is, so it does not get traded for
    /// a tidier library. It carries the library count and the last status line, and the dot repeats
    /// the colour from the top bar so the two cannot disagree.
    private var statusStrip: some View {
        Button {
            host.showDiagnostics.toggle()
        } label: {
            HStack(spacing: 8) {
                Circle()
                    .fill(StatusTint.tint(for: host.status))
                    .frame(width: 6, height: 6)
                Text("\(host.library.count) titles in library \u{00B7} \(host.status)")
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(Color.white.opacity(0.66))
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 4)
                Image(systemName: host.showDiagnostics ? "chevron.down" : "chevron.up")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(Color.white.opacity(0.5))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .overlay(alignment: .top) {
            Rectangle()
                .fill(ShellPalette.hairline)
                .frame(height: 0.5)
        }
    }

    private var tabBar: some View {
        HStack(spacing: 0) {
            ForEach(LibraryTab.allCases) { item in
                let selected = host.libraryTab == item
                Button {
                    host.libraryTab = item
                } label: {
                    Text(item.title)
                        .font(.system(size: 15, weight: selected ? .semibold : .regular))
                        .foregroundStyle(selected ? ShellPalette.accent
                                                  : Color.white.opacity(0.62))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
            }
        }
        .padding(.bottom, max(metrics.insets.bottom, 6))
        .overlay(alignment: .top) {
            Rectangle()
                .fill(ShellPalette.hairline)
                .frame(height: 0.5)
        }
    }

    // MARK: The tabs

    @ViewBuilder
    private var content: some View {
        switch host.libraryTab {
        case .home:
            homeTab
        case .allGames:
            allGamesTab
        case .favorites:
            favoritesTab
        case .settings:
            settingsTab
        }
    }

    /// Home: the hero, then the shelves.
    private var homeTab: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 22) {
                if let hero = heroEntry {
                    HeroCard(
                        entry: hero,
                        system: CoreCatalog.system(forExtension: hero.ext),
                        store: artwork,
                        generation: artwork.generation,
                        isFavourite: host.favourites.contains(hero.id),
                        height: heroHeight,
                        host: host
                    )
                } else {
                    EmptyLibraryNotice(
                        title: "Nothing to play yet",
                        guidance: Self.emptyGuidance,
                        actionTitle: "Import games"
                    ) {
                        host.presentImportPicker()
                    }
                    .padding(.top, topBarHeight + max(metrics.insets.top, 8) + 12)
                }

                ForEach(shelves) { shelf in
                    ShelfRow(
                        title: shelf.title,
                        entries: shelf.entries,
                        store: artwork,
                        generation: artwork.generation,
                        favourites: host.favourites,
                        host: host
                    )
                }

                Spacer(minLength: bottomBarHeight)
            }
        }
        // The hero runs under the top bar on purpose, which is what full-bleed means here.
        .ignoresSafeArea(edges: .top)
    }

    /// All Games: everything, searchable, in whichever layout the user chose, and the one place a
    /// game can be deleted by swiping.
    @ViewBuilder
    private var allGamesTab: some View {
        let shown = filtered(host.library)
        VStack(alignment: .leading, spacing: 10) {
            tabHeader(
                title: "All Games",
                subtitle: subtitle(for: shown, of: host.library.count)
            )

            if host.library.isEmpty {
                EmptyLibraryNotice(title: "Nothing imported yet",
                                   guidance: Self.emptyGuidance,
                                   actionTitle: "Import games") {
                    host.presentImportPicker()
                }
                Spacer(minLength: 0)
            } else if shown.isEmpty {
                EmptyLibraryNotice(
                    title: "No match",
                    guidance: "Nothing in the library matches \"\(host.librarySearch)\". Search "
                        + "runs over the filename on disk and the title derived from it."
                )
                Spacer(minLength: 0)
            } else if host.libraryLayout == .grid {
                gameGrid(shown)
            } else {
                gameList(shown)
            }
        }
        .padding(.top, topBarHeight + max(metrics.insets.top, 8) + 6)
    }

    /// Favorites: the same cards, and an empty state that says how to favourite something.
    @ViewBuilder
    private var favoritesTab: some View {
        let shown = filtered(host.favouriteEntries)
        VStack(alignment: .leading, spacing: 10) {
            tabHeader(
                title: "Favorites",
                subtitle: host.favouriteEntries.isEmpty
                    ? "nothing favourited yet"
                    : subtitle(for: shown, of: host.favouriteEntries.count)
            )

            if host.favouriteEntries.isEmpty {
                EmptyLibraryNotice(
                    title: "No favorites yet",
                    guidance: "Press and hold a game to open its detail sheet and tap the star, or "
                        + "swipe a row right in All Games. Favorites are remembered by the game's "
                        + "path on disk, so they survive a relaunch, and a file that is "
                        + "temporarily missing is not forgotten."
                )
                Spacer(minLength: 0)
            } else if shown.isEmpty {
                EmptyLibraryNotice(
                    title: "No match",
                    guidance: "No favorite matches \"\(host.librarySearch)\"."
                )
                Spacer(minLength: 0)
            } else {
                gameGrid(shown)
            }
        }
        .padding(.top, topBarHeight + max(metrics.insets.top, 8) + 6)
    }

    private var settingsTab: some View {
        SettingsScreen(host: host, artwork: artwork)
            .padding(.top, topBarHeight + max(metrics.insets.top, 8) + 6)
            .padding(.bottom, bottomBarHeight)
    }

    // MARK: Pieces the tabs share

    private func tabHeader(title: String, subtitle: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.system(size: 26, weight: .heavy))
                .foregroundStyle(.white)
            Text(subtitle)
                .font(.system(size: 12))
                .foregroundStyle(ShellPalette.secondaryText)
            Spacer(minLength: 0)
            if host.libraryTab == .allGames {
                layoutToggle
            }
        }
        .padding(.horizontal, 16)
    }

    /// The layout switch, in reach rather than buried: it is also in Settings, and both write the
    /// same persisted value.
    private var layoutToggle: some View {
        Button {
            host.libraryLayout = host.libraryLayout == .grid ? .list : .grid
        } label: {
            Image(systemName: host.libraryLayout == .grid
                  ? "square.grid.2x2.fill"
                  : "list.bullet")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 34, height: 34)
                .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 9))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(host.libraryLayout == .grid ? "Switch to a list" : "Switch to a grid")
    }

    private func gameGrid(_ entries: [LibraryEntry]) -> some View {
        ScrollView {
            LazyVGrid(
                // Adaptive so one grid works in portrait and landscape and on an iPad, rather than
                // a fixed column count that would be wrong in one of them.
                columns: [GridItem(.adaptive(minimum: 104, maximum: 150), spacing: 12)],
                alignment: .leading,
                spacing: 16
            ) {
                ForEach(entries) { entry in
                    CoverCard(
                        entry: entry,
                        system: CoreCatalog.system(forExtension: entry.ext),
                        store: artwork,
                        generation: artwork.generation,
                        isFavourite: host.favourites.contains(entry.id),
                        width: 110,
                        host: host,
                        showsLabel: true
                    )
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, bottomBarHeight)
        }
    }

    private func gameList(_ entries: [LibraryEntry]) -> some View {
        List {
            // Identity is `LibraryEntry.id`, the absolute path, so rows keep their identity across
            // the rescan that follows every import and delete.
            ForEach(entries) { entry in
                // A tap gesture on the row rather than a Button wrapping it, because the row
                // carries its own info Button and a control inside a Button's label is not
                // interactive: the outer Button swallows the tap and the info affordance would
                // silently do nothing.
                GameListRow(
                    entry: entry,
                    system: CoreCatalog.system(forExtension: entry.ext),
                    store: artwork,
                    generation: artwork.generation,
                    isFavourite: host.favourites.contains(entry.id),
                    host: host
                )
                .onTapGesture {
                    host.launch(entry: entry)
                }
                .onLongPressGesture(minimumDuration: 0.4) {
                    host.detailEntry = entry
                }
                .listRowInsets(EdgeInsets(top: 6, leading: 12, bottom: 6, trailing: 10))
                .listRowBackground(Color.white.opacity(0.05))
                .listRowSeparatorTint(ShellPalette.hairline)
                .swipeActions(edge: .leading, allowsFullSwipe: true) {
                    Button {
                        host.toggleFavourite(entry)
                    } label: {
                        Label(host.favourites.contains(entry.id) ? "Unstar" : "Star",
                              systemImage: "star")
                    }
                    .tint(.yellow)
                }
            }
            .onDelete { offsets in
                // Resolved to ids and then back to indices in `host.library` before being handed
                // over, because these offsets index the FILTERED list while `deleteEntries(at:)`
                // indexes the real one. Deleting by a filtered index would remove a different game,
                // which is the exact class of bug that keeping identity on the path rules out.
                let ids = Set(offsets.compactMap { index -> String? in
                    entries.indices.contains(index) ? entries[index].id : nil
                })
                host.deleteEntries(at: host.indices(matching: ids))
            }
            Spacer(minLength: bottomBarHeight)
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
    }

    // MARK: What each tab shows

    /// Search runs over the filename on disk and over the title derived from it.
    ///
    /// The filename is the only text this app reliably has, so it is the primary target; the
    /// derived title is included because a user who types "mario" should not miss a file called
    /// "Super Mario World (USA) [!].sfc" on account of the tags, and the title is DERIVED from that
    /// filename rather than invented, so it adds no knowledge the app does not have.
    private func filtered(_ entries: [LibraryEntry]) -> [LibraryEntry] {
        let needle = host.librarySearch.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !needle.isEmpty else { return entries }
        return entries.filter { entry in
            entry.name.lowercased().contains(needle)
                || GameMetadata.displayTitle(for: entry).lowercased().contains(needle)
        }
    }

    private func subtitle(for shown: [LibraryEntry], of total: Int) -> String {
        if shown.count == total {
            return total == 1 ? "1 title" : "\(total) titles"
        }
        return "\(shown.count) of \(total) titles"
    }

    /// The featured game: the most recently added one.
    ///
    /// Deterministic on purpose. A random hero would change on every rescan, and a rescan happens
    /// after every import and every delete. When no file carries a date the first game by name is
    /// used, which is stable for the same reason.
    private var heroEntry: LibraryEntry? {
        host.library.max { left, right in
            switch (left.addedAt, right.addedAt) {
            case let (leftDate?, rightDate?):
                if leftDate == rightDate {
                    return left.name.localizedStandardCompare(right.name) == .orderedDescending
                }
                return leftDate < rightDate
            case (nil, _?):
                return true
            case (_?, nil):
                return false
            case (nil, nil):
                return left.name.localizedStandardCompare(right.name) == .orderedDescending
            }
        }
    }

    /// One shelf, ready to render.
    private struct Shelf: Identifiable {
        let id: String
        let title: String
        let entries: [LibraryEntry]
    }

    /// Recently added, then one shelf per system that has games.
    ///
    /// Per-system is honest grouping: it needs no metadata the app does not have, unlike a "Because
    /// you played" or a genre row, which would be invented. The per-system shelves can be turned
    /// off in Settings, which is part of what the user asked for when they said they wanted to
    /// change the layout.
    private var shelves: [Shelf] {
        guard !host.library.isEmpty else { return [] }
        var out: [Shelf] = []

        let recent = host.library
            .sorted { left, right in
                switch (left.addedAt, right.addedAt) {
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
        out.append(Shelf(id: "recent", title: "Recently added",
                         entries: Array(recent.prefix(18))))

        guard host.showsSystemShelves else { return out }

        // Grouped through the ONE routing table, so a shelf cannot claim a system the launch path
        // would disagree with. Ordered by the system's own case order, so the shelves do not
        // reshuffle when a game is added.
        var bySystem: [GameSystem: [LibraryEntry]] = [:]
        var unrouted: [LibraryEntry] = []
        for entry in host.library {
            if let system = CoreCatalog.system(forExtension: entry.ext) {
                bySystem[system, default: []].append(entry)
            } else {
                unrouted.append(entry)
            }
        }
        for system in GameSystem.allCases {
            guard let entries = bySystem[system], !entries.isEmpty else { continue }
            out.append(Shelf(id: system.rawValue, title: system.displayName, entries: entries))
        }
        if !unrouted.isEmpty {
            // Should be unreachable, because the library only lists launchable extensions and every
            // one of those is in the routing table. It is a shelf rather than a silent drop so that
            // if it ever is reachable, it is visible instead of losing games.
            out.append(Shelf(id: "unrouted", title: "No core mapped", entries: unrouted))
        }
        return out
    }
}

// MARK: - The real safe-area insets

/// What the shell needs to know about the window it is in.
struct ShellMetrics: Equatable {
    var insets = UIEdgeInsets.zero
    var size = CGSize.zero
}

/// Reports the window's safe-area insets and size into SwiftUI.
///
/// Needed because the app's root applies `.ignoresSafeArea()` to the whole window group, so a
/// GeometryProxy inside it reports zero insets: correct for the Metal canvas, which wants the full
/// screen, and useless for the chrome, which has to stay clear of the sensor housing and the home
/// indicator. A UIView always knows the truth, which is how the touch controls get theirs, so the
/// shell asks the same way. The size comes from the same place rather than from `UIScreen.main`,
/// which is the wrong answer on a device that can put this app in a smaller window.
struct SafeAreaProbe: UIViewRepresentable {
    let onChange: (ShellMetrics) -> Void

    func makeUIView(context: Context) -> ProbeView {
        let view = ProbeView()
        view.onChange = onChange
        return view
    }

    func updateUIView(_ view: ProbeView, context: Context) {
        view.onChange = onChange
    }

    final class ProbeView: UIView {
        var onChange: ((ShellMetrics) -> Void)?
        private var reported: ShellMetrics?

        override func layoutSubviews() {
            super.layoutSubviews()
            report()
        }

        override func safeAreaInsetsDidChange() {
            super.safeAreaInsetsDidChange()
            report()
        }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            report()
        }

        private func report() {
            // The WINDOW's values, not this view's. This probe sits in a background layer that may
            // itself already be inside a padded container, where its own insets are zero.
            let metrics = ShellMetrics(
                insets: window?.safeAreaInsets ?? safeAreaInsets,
                size: window?.bounds.size ?? bounds.size
            )
            guard metrics != reported else { return }
            reported = metrics
            // Hopped to the next turn of the run loop, because this arrives during a UIKit layout
            // pass and writing SwiftUI state inside one is how an update loop starts. The equality
            // check above is what stops this becoming a per-layout write.
            let send = onChange
            DispatchQueue.main.async {
                send?(metrics)
            }
        }
    }
}
