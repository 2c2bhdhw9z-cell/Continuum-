// Continuum - the in-game pause menu in the new look (Settings, LOOK, New).
//
// The current look keeps its row of round buttons and the ⋯ menu. The new look puts the common
// session actions in a dark glass sheet with gold accents, opened from a gold pause button that
// pauses the game. Every tile calls the action the current look already has; the sheet chooses,
// and `PlayerScreen.finishPauseMenu` runs the choice once the sheet has gone, so a second sheet
// (save slots, cheats, a skin) never tries to open over this one.

import SwiftUI

enum PauseChoice: String {
    case resume
    case saveState
    case loadLast
    case slots
    case reset
    case swapScreens
    case cheats
    case skin
    case functions
    case controllers
    case netplay
    case cover
    case feedback
    case details
    case quit
}

struct NewLookPauseSheet: View {
    let host: EngineHost
    @ObservedObject var saveStates: SaveStates
    let entry: LibraryEntry?
    let system: GameSystem?
    let netplayLive: Bool
    let isWebPlayer: Bool
    let twoScreens: Bool
    let showsDetails: Bool
    let onChoose: (PauseChoice) -> Void

    private struct Tile: Identifiable {
        let id: PauseChoice
        let icon: String
        let title: String
        var detail: String?
    }

    private var newestState: SaveStateRecord? {
        guard let entry else { return nil }
        return saveStates.states(for: entry).first
    }

    /// Only the actions that can work right now, the way the current look hides them too:
    /// no save states for the bundled players, no load, reset or rewind during online play.
    private var tiles: [Tile] {
        var out: [Tile] = []
        if !isWebPlayer {
            out.append(Tile(id: .saveState, icon: "square.and.arrow.down.fill", title: "Save state"))
            if !netplayLive, let newest = newestState {
                out.append(Tile(id: .loadLast, icon: "clock.arrow.circlepath", title: "Load last",
                                detail: newest.ageText))
            }
            out.append(Tile(id: .slots, icon: "square.grid.2x2.fill", title: "Save slots"))
        }
        if !netplayLive {
            out.append(Tile(id: .reset, icon: "arrow.clockwise", title: "Reset"))
        }
        if twoScreens {
            out.append(Tile(id: .swapScreens, icon: "rectangle.2.swap", title: "Swap screens"))
        }
        if !isWebPlayer {
            out.append(Tile(id: .cheats, icon: "wand.and.stars", title: "Cheats"))
        }
        out.append(Tile(id: .skin, icon: "paintpalette.fill", title: "Skin"))
        out.append(Tile(id: .controllers, icon: "gamecontroller.fill", title: "Controllers"))
        if !isWebPlayer {
            out.append(Tile(id: .netplay, icon: "person.2.fill",
                            title: netplayLive ? "Online play" : "Play online"))
            out.append(Tile(id: .cover, icon: "camera.viewfinder", title: "Use as cover"))
        }
        out.append(Tile(id: .functions, icon: "square.grid.3x3.fill", title: "All functions"))
        out.append(Tile(id: .feedback, icon: "envelope.fill", title: "Feedback"))
        out.append(Tile(id: .details, icon: "info.circle.fill",
                        title: showsDetails ? "Hide details" : "Tech details"))
        return out
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header

                NewLookPrimaryButton(title: "Resume", systemImage: "play.fill") {
                    onChoose(.resume)
                }
                .frame(maxWidth: .infinity)

                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 10), count: 3),
                          spacing: 10) {
                    ForEach(tiles) { tile in
                        tileView(tile)
                    }
                }

                Button {
                    onChoose(.quit)
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "xmark.circle.fill")
                        Text("Quit to library")
                    }
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(ShellPalette.danger)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 13)
                    .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 14))
                    .overlay(
                        RoundedRectangle(cornerRadius: 14)
                            .strokeBorder(ShellPalette.danger.opacity(0.45), lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
            }
            .padding(18)
        }
        .background(
            ZStack {
                ShellPalette.charcoal.opacity(0.80)
                RadialGradient(colors: [ShellPalette.gold.opacity(0.14), .clear],
                               center: .top, startRadius: 10, endRadius: 360)
            }
            .ignoresSafeArea()
        )
        .modifier(GlassSheetBackground())
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .preferredColorScheme(.dark)
    }

    private var header: some View {
        HStack(spacing: 14) {
            if let entry {
                CoverArtView(entry: entry, system: system, store: host.artwork,
                             generation: host.artwork.generation, showsCaption: false)
                    .frame(width: 54, height: 72)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .strokeBorder(ShellPalette.gold.opacity(0.35), lineWidth: 1)
                    )
            }
            VStack(alignment: .leading, spacing: 4) {
                Text("PAUSED")
                    .font(.system(size: 12, weight: .bold))
                    .tracking(1.4)
                    .foregroundStyle(ShellPalette.gold)
                Text(entry.map { GameMetadata.displayTitle(for: $0) } ?? "No game")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(.white)
                    .lineLimit(2)
                if let entry {
                    Text(NewLook.metaLine(entry: entry, system: system,
                                          record: PlayHistory.shared.record(for: entry.name),
                                          ps1CoreId: host.ps1CoreChoice.coreId))
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(Color.white.opacity(0.6))
                        .lineLimit(2)
                }
                if netplayLive {
                    Text("Online play keeps running while this menu is open.")
                        .font(.system(size: 11))
                        .foregroundStyle(Color.white.opacity(0.6))
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func tileView(_ tile: Tile) -> some View {
        Button {
            onChoose(tile.id)
        } label: {
            VStack(spacing: 7) {
                Image(systemName: tile.icon)
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(ShellPalette.gold)
                    .frame(height: 24)
                Text(tile.title)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                if let detail = tile.detail {
                    Text(detail)
                        .font(.system(size: 10))
                        .foregroundStyle(Color.white.opacity(0.55))
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                }
            }
            .frame(maxWidth: .infinity, minHeight: 82)
            .padding(.horizontal, 4)
            .background(Color.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 16))
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(tile.detail.map { "\(tile.title), \($0)" } ?? tile.title)
    }
}

/// Real glass behind the sheet on iOS 16.4 and later, where a sheet's own background can be
/// replaced; on 16.0 to 16.3 the sheet keeps the system's dark background under the charcoal.
private struct GlassSheetBackground: ViewModifier {
    @ViewBuilder
    func body(content: Content) -> some View {
        if #available(iOS 16.4, *) {
            content.presentationBackground(.ultraThinMaterial)
        } else {
            content
        }
    }
}
