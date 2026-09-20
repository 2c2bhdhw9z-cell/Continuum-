// Continuum - one game's detail sheet: what it is, where its cover came from, and what can be done
// to it.
//
// A sheet rather than a pushed view, for the reason the shell's header gives: the Metal canvas is
// mounted once for the app's lifetime and a navigation push that replaced it would tear down the
// layer the engine attached to. A sheet is drawn above the existing tree and disturbs nothing.
//
// This is also where the honest detail lives. The design reference's detail view shows a
// description and a rating; this one shows the filename on disk, the real total size of a
// multi-file game, which cue tracks are missing, and which core the tap will route to, because
// those are the four things this app actually knows and every one of them has explained a failure
// at least once.

import SwiftUI

struct GameDetailSheet: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var artwork: ArtworkStore

    private var system: GameSystem? {
        CoreCatalog.system(forExtension: entry.ext)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                actions
                facts
                artworkBlock
                dangerZone
            }
            .padding(18)
        }
        .background(Color.black.ignoresSafeArea())
        .presentationDragIndicator(.visible)
    }

    // MARK: Header

    private var header: some View {
        HStack(alignment: .top, spacing: 14) {
            CoverArtView(entry: entry, system: system, store: artwork,
                         generation: artwork.generation, showsCaption: true)
                .frame(width: 118, height: 157)
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(Color.white.opacity(0.12), lineWidth: 0.5)
                )

            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .top, spacing: 6) {
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 21, weight: .heavy))
                        .foregroundStyle(.white)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 0)
                    FavouriteButton(isFavourite: host.favourites.contains(entry.id), size: 17) {
                        host.toggleFavourite(entry)
                    }
                }
                Text(GameMetadata.metaLine(for: entry, system: system))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(ShellPalette.metadata)
                    .fixedSize(horizontal: false, vertical: true)
                if let system {
                    Text(system.displayName)
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
                Spacer(minLength: 0)
            }
        }
    }

    // MARK: Actions

    private var actions: some View {
        HStack(spacing: 10) {
            PillButton(title: "Play", systemImage: "play.fill", filled: true) {
                // The one launch path, unchanged. Clearing the binding is what dismisses the sheet,
                // so the player screen is not opened underneath it.
                let target = entry
                host.detailEntry = nil
                host.launch(entry: target)
            }
            PillButton(title: "Close", systemImage: nil, filled: false) {
                host.detailEntry = nil
            }
            Spacer(minLength: 0)
        }
    }

    // MARK: The facts

    private var facts: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsReadout(label: "On disk as", value: entry.name)
            SettingsReadout(label: "Detail", value: entry.detail)
            SettingsReadout(
                label: "Runs on",
                value: CoreCatalog.core(forExtension: entry.ext)?.displayName
                    ?? "no core is mapped to .\(entry.ext)"
            )
            if !entry.cueNotes.isEmpty {
                // The commonest import mistake by a distance: bringing the .cue and leaving its
                // .bin tracks behind. Said out loud here rather than only as part of the detail
                // line.
                SettingsNote(
                    "This cue sheet is incomplete: "
                    + entry.cueNotes.joined(separator: ", ")
                    + ". A PlayStation game is a .cue plus every .bin track it names, so import "
                    + "the missing tracks with the same names and it will boot."
                )
            }
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }

    // MARK: Artwork

    private var artworkBlock: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("COVER ART")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)

            SettingsReadout(
                label: "Source",
                value: artwork.provenance(for: entry)
                    ?? (artwork.fetchEnabled
                        ? "no cover stored yet, so the generated plate is showing"
                        : "artwork lookups are off, so the generated plate is showing")
            )

            SettingsButton(title: "Choose an image from Files", role: .normal) {
                artwork.presentArtworkPicker(for: entry)
            }
            SettingsButton(title: "Look it up again", role: .normal) {
                artwork.lookUpAgain(entry)
            }
            SettingsButton(title: "Remove the stored cover", role: .destructive) {
                artwork.clearCover(for: entry)
            }
            SettingsNote(
                "A chosen image wins over anything downloaded, which is the answer for a game the "
                + "thumbnail database has never heard of. The generated plate is always "
                + "underneath, so nothing is ever blank."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }

    // MARK: Delete

    private var dangerZone: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsButton(title: "Delete this game", role: .destructive) {
                // Routed through the one delete implementation, by resolving this entry's id back
                // to its index in the live library array rather than passing an index the sheet
                // made up.
                let indices = host.indices(matching: [entry.id])
                host.detailEntry = nil
                host.deleteEntries(at: indices)
            }
            SettingsNote(
                "Deletes the file from the Continuum folder. If this is a cue sheet, the .bin "
                + "tracks it named stay on disk: working out which tracks belonged to it means "
                + "parsing the sheet, and a wrong guess deletes a track another game needs."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }
}
