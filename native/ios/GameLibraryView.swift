//
//  GameLibraryView.swift
//  Continuum
//
//  Example: Replicating the PWA library UI in SwiftUI
//

import SwiftUI

// MARK: - Game Model

struct Game: Identifiable {
    let id: String
    let title: String
    let system: String
    let coreId: String
    let romPath: String
    let artworkURL: URL?
    let needsFullPath: Bool

    var systemDisplayName: String {
        switch system {
        case "ps1": return "PlayStation"
        case "nes": return "NES"
        case "snes": return "Super Nintendo"
        case "gba": return "Game Boy Advance"
        case "genesis": return "Sega Genesis"
        default: return system.uppercased()
        }
    }
}

// MARK: - Library View (replaces PWA's library-view.js)

struct GameLibraryView: View {
    @StateObject private var viewModel = GameLibraryViewModel()
    @State private var searchText = ""
    @State private var selectedTab = 0

    var body: some View {
        NavigationView {
            ZStack {
                Color.black.ignoresSafeArea()

                VStack(spacing: 0) {
                    // Top bar (like PWA's topbar)
                    LibraryTopBar(
                        searchText: $searchText,
                        selectedTab: $selectedTab,
                        onImport: viewModel.importROM
                    )

                    // Content
                    if viewModel.games.isEmpty {
                        EmptyLibraryView(onImport: viewModel.importROM)
                    } else {
                        ScrollView {
                            if selectedTab == 0 {
                                // Home - shelves by system
                                ShelvesView(games: filteredGames, onPlay: viewModel.launchGame)
                            } else if selectedTab == 1 {
                                // All games - grid
                                GameGridView(games: filteredGames, onPlay: viewModel.launchGame)
                            }
                        }
                    }
                }
            }
            .navigationBarHidden(true)
        }
        .preferredColorScheme(.dark)
    }

    var filteredGames: [Game] {
        if searchText.isEmpty {
            return viewModel.games
        }
        return viewModel.games.filter { $0.title.localizedCaseInsensitiveContains(searchText) }
    }
}

// MARK: - Top Bar

struct LibraryTopBar: View {
    @Binding var searchText: String
    @Binding var selectedTab: Int
    let onImport: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Brand + Search
            HStack {
                Text("CONTINUUM")
                    .font(.system(.title2, design: .monospaced))
                    .fontWeight(.bold)
                    .foregroundColor(.white)

                Spacer()

                // Search
                HStack {
                    Image(systemName: "magnifyingglass")
                        .foregroundColor(.gray)
                    TextField("Search library...", text: $searchText)
                        .textFieldStyle(.plain)
                        .foregroundColor(.white)
                }
                .padding(8)
                .background(Color.white.opacity(0.1))
                .cornerRadius(8)
                .frame(maxWidth: 300)

                // Add ROM button
                Button(action: onImport) {
                    Label("Add ROM", systemImage: "plus")
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                        .background(Color.blue)
                        .foregroundColor(.white)
                        .cornerRadius(8)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 12)

            // Tabs
            HStack(spacing: 24) {
                TabButton(title: "Home", isSelected: selectedTab == 0) {
                    selectedTab = 0
                }
                TabButton(title: "All Games", isSelected: selectedTab == 1) {
                    selectedTab = 1
                }
                TabButton(title: "Favorites", isSelected: selectedTab == 2) {
                    selectedTab = 2
                }
                Spacer()
            }
            .padding(.horizontal)
            .padding(.bottom, 8)
        }
        .background(Color(white: 0.05))
    }
}

struct TabButton: View {
    let title: String
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(.subheadline, design: .default))
                .fontWeight(isSelected ? .semibold : .regular)
                .foregroundColor(isSelected ? .white : .gray)
                .padding(.bottom, 4)
                .overlay(
                    Rectangle()
                        .fill(isSelected ? Color.blue : Color.clear)
                        .frame(height: 2),
                    alignment: .bottom
                )
        }
    }
}

// MARK: - Shelves View (like PWA's shelf rows)

struct ShelvesView: View {
    let games: [Game]
    let onPlay: (Game) -> Void

    var gamesBySystem: [String: [Game]] {
        Dictionary(grouping: games, by: { $0.system })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 32) {
            ForEach(gamesBySystem.keys.sorted(), id: \.self) { system in
                if let systemGames = gamesBySystem[system], !systemGames.isEmpty {
                    ShelfRow(
                        title: systemGames.first?.systemDisplayName ?? system.uppercased(),
                        games: systemGames,
                        onPlay: onPlay
                    )
                }
            }
        }
        .padding()
    }
}

struct ShelfRow: View {
    let title: String
    let games: [Game]
    let onPlay: (Game) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.title3)
                .fontWeight(.semibold)
                .foregroundColor(.white)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 16) {
                    ForEach(games) { game in
                        GameCard(game: game, onPlay: { onPlay(game) })
                    }
                }
            }
        }
    }
}

// MARK: - Game Card (like PWA's card.js)

struct GameCard: View {
    let game: Game
    let onPlay: () -> Void

    var body: some View {
        Button(action: onPlay) {
            VStack(alignment: .leading, spacing: 8) {
                // Artwork
                if let artworkURL = game.artworkURL {
                    AsyncImage(url: artworkURL) { image in
                        image.resizable().aspectRatio(contentMode: .fill)
                    } placeholder: {
                        PlaceholderArt(system: game.system)
                    }
                    .frame(width: 160, height: 240)
                    .cornerRadius(8)
                } else {
                    PlaceholderArt(system: game.system)
                        .frame(width: 160, height: 240)
                        .cornerRadius(8)
                }

                // Title
                Text(game.title)
                    .font(.system(.caption, design: .default))
                    .foregroundColor(.white)
                    .lineLimit(2)
                    .frame(width: 160, alignment: .leading)

                // System badge
                Text(game.systemDisplayName)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundColor(.gray)
            }
        }
    }
}

struct PlaceholderArt: View {
    let system: String

    var systemColor: Color {
        switch system {
        case "ps1": return .purple
        case "nes": return .red
        case "snes": return .purple
        case "gba": return .blue
        case "genesis": return .black
        default: return .gray
        }
    }

    var body: some View {
        ZStack {
            systemColor
            VStack {
                Image(systemName: "gamecontroller.fill")
                    .font(.system(size: 40))
                    .foregroundColor(.white.opacity(0.5))
                Text(system.uppercased())
                    .font(.system(.headline, design: .monospaced))
                    .foregroundColor(.white.opacity(0.7))
            }
        }
    }
}

// MARK: - Grid View (like PWA's all-games grid)

struct GameGridView: View {
    let games: [Game]
    let onPlay: (Game) -> Void

    let columns = [
        GridItem(.adaptive(minimum: 160), spacing: 16)
    ]

    var body: some View {
        LazyVGrid(columns: columns, spacing: 24) {
            ForEach(games) { game in
                GameCard(game: game, onPlay: { onPlay(game) })
            }
        }
        .padding()
    }
}

// MARK: - Empty State

struct EmptyLibraryView: View {
    let onImport: () -> Void

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "gamecontroller")
                .font(.system(size: 80))
                .foregroundColor(.gray)

            Text("No Games Yet")
                .font(.title)
                .foregroundColor(.white)

            Text("Add ROMs from your device to get started")
                .font(.body)
                .foregroundColor(.gray)
                .multilineTextAlignment(.center)

            Button(action: onImport) {
                Label("Add ROM", systemImage: "plus")
                    .padding(.horizontal, 24)
                    .padding(.vertical, 12)
                    .background(Color.blue)
                    .foregroundColor(.white)
                    .cornerRadius(12)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - ViewModel

@MainActor
class GameLibraryViewModel: ObservableObject {
    @Published var games: [Game] = []

    private let engine: ContinuumEngine
    private let coreManager: CoreManager

    init() {
        self.engine = ContinuumEngine()
        self.coreManager = CoreManager(engine: engine)

        // Declare cores
        try? coreManager.declareAllCores()

        // Load saved games from Documents
        loadLibrary()
    }

    func loadLibrary() {
        // Scan Documents directory for imported ROMs
        guard let documentsURL = FileManager.default.urls(
            for: .documentDirectory,
            in: .userDomainMask
        ).first else { return }

        do {
            let files = try FileManager.default.contentsOfDirectory(
                at: documentsURL,
                includingPropertiesForKeys: nil
            )

            games = files.compactMap { url -> Game? in
                guard let system = detectSystem(from: url.pathExtension),
                      let coreId = coreForSystem(system) else { return nil }

                return Game(
                    id: url.lastPathComponent,
                    title: url.deletingPathExtension().lastPathComponent,
                    system: system,
                    coreId: coreId,
                    romPath: url.path,
                    artworkURL: nil,
                    needsFullPath: url.pathExtension == "cue" || url.pathExtension == "iso"
                )
            }
        } catch {
            print("Failed to load library: \(error)")
        }
    }

    func importROM() {
        // Present UIDocumentPickerViewController
        // Copy selected file(s) to Documents directory
        // Reload library
        print("Import ROM tapped - implement UIDocumentPickerViewController")
    }

    func launchGame(_ game: Game) {
        do {
            try coreManager.launchGame(
                coreId: game.coreId,
                contentId: game.id,
                romPath: game.romPath,
                needsFullPath: game.needsFullPath
            )
            // Navigate to player view
            print("✓ Launched \(game.title)")
        } catch {
            print("Failed to launch \(game.title): \(error)")
        }
    }

    private func detectSystem(from ext: String) -> String? {
        switch ext.lowercased() {
        case "nes": return "nes"
        case "sfc", "smc": return "snes"
        case "gba": return "gba"
        case "gb", "gbc": return "gb"
        case "gen", "md": return "genesis"
        case "sms": return "sms"
        case "cue", "bin", "iso": return "ps1"
        default: return nil
        }
    }

    private func coreForSystem(_ system: String) -> String? {
        switch system {
        case "ps1": return "pcsx_rearmed"
        case "nes": return "fceumm"
        case "snes": return "snes9x"
        case "gba", "gb": return "mgba"
        case "genesis", "sms": return "genesis_plus_gx"
        default: return nil
        }
    }
}
